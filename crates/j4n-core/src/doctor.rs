//! The pre-flight: what J4n has to work with — each doer found, signed in or missing and the one
//! line that mends it, the plans and whether they have a price, whether the gateway key is set, and the safety defaults in words.
//! The facts are gathered where the doers are known (the binary, from their own probes); what they say and the exit status are decided
//! here, so `j4n doctor`, the Setup page and the notice of a new folder always tell the same.

use crate::clock::{Clock, SystemClock};
use crate::doer::Probe;
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// The doers the pre-flight names, in the order it says them.
pub const DOERS: [&str; 7] = ["claude", "codex", "gemini", "opencode", "pi", "own", "light"];

/// The pack that declares a doer, for the line that says why it is not declared.
pub fn pack_of(id: &str) -> &'static str {
    match id {
        "codex" | "gemini" | "opencode" => "connect",
        "own" => "agent",
        "light" => "jev",
        _ => "base",
    }
}

/// What a setting that names a doer says when there is none of that name (`shape.model`, `ledger.model`): where the doer comes from, so the
/// operator turns on the pack that declares it, or names another.
pub fn no_doer(what: &str, id: &str) -> String {
    match pack_of(id) {
        "base" => format!("{what} names `{id}`, and there is no such doer"),
        pack => format!("{what} names `{id}`, and there is no such doer: the pack {pack} declares it, and it is off · Grow › Packs › {pack}: ⏎ turns it on"),
    }
}

/// One doer as its probe found it.
#[derive(Debug, Clone, Default)]
pub struct DoerFact {
    pub id: String,
    /// What its own probe said; none: nothing of that name is declared (its pack is off).
    pub probe: Option<Probe>,
    /// It needs a key (a model on a gateway), not a program on the PATH.
    pub keyed: bool,
    /// An election for `work` may pick it (its roles are none, or name `work`).
    pub work: bool,
    /// A sign-in an account can keep (`:account add`).
    pub account: bool,
    /// The line that mends it when it is not ready.
    pub fix: String,
    /// How long its probe took, in milliseconds: what starting it costs before any request; none: it was not probed.
    pub ms: Option<u64>,
    /// Its probe has not answered yet: what it says is not known, which is neither "not found" nor "not signed in".
    pub pending: bool,
}

/// One plan row: its price, when the operator gave it.
#[derive(Debug, Clone, Default)]
pub struct PlanFact {
    pub id: String,
    pub price: Option<f64>,
    /// `month` or `week`.
    pub per: &'static str,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub doers: Vec<DoerFact>,
    pub plans: Vec<PlanFact>,
    /// The OpenRouter key is there (never its value), and the line that says where to put it when it is not.
    pub key: bool,
    pub key_fix: String,
    /// The doer the operator picked for the main lane (`lane.doer`); the default is it when it is ready.
    pub pick: Option<String>,
    /// The packs that are on and are not J4n basic, and why: `(id, the reason)`: the folder already uses a feature of the pack, an `enable` row
    /// of the operator's, `packs.full`, or another pack needs it.
    pub kept: Vec<(String, String)>,
    /// `packs.full` is on: the doctor says so, and the setting is not listed off.
    pub full: bool,
    /// The switches `packs.full` turned on, `(setting, what it holds)`: those the operator's own rows did not take back.
    pub full_sets: Vec<(String, String)>,
    /// The word of `lanes` is `orchestrate`: the look, the crew and the envoys follow it, so they are not listed off.
    pub orchestrating: bool,
    /// What a feature that is on lacks: the need, what is missing, and what was found and where to set it.
    pub needs: Vec<NeedFact>,
}

/// One feature that is on and lacks what it needs.
#[derive(Debug, Clone, Default)]
pub struct NeedFact {
    pub id: String,
    pub what: String,
    pub fix: String,
}

/// One line of the report, which a page shows as a row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// `doer`, `key`, `plan`, `need`, `safe`, `off` or `default`.
    pub kind: &'static str,
    pub id: String,
    /// `good`, `warn` or `bad`: the row's tone.
    pub tone: &'static str,
    pub what: String,
    pub fix: String,
    pub account: bool,
    /// A doer's probe time in milliseconds.
    pub ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub rows: Vec<Row>,
    /// The doer the first election falls to (the ready ones that take `work`, the smallest id: `rank.rs`); none: none works, or it is not
    /// known yet (`checking`).
    pub default: Option<String>,
    /// A doer's probe has not answered yet: the rows of those say so, and a caller that acts on the report waits ([`Check::report_now`]).
    pub checking: bool,
}

/// What a row says while its probe has not answered.
pub const CHECKING: &str = "checking…";

/// What each doer says and the doer a first election falls to.
pub fn report(f: &Facts) -> Report {
    let row = |kind, id: &str, tone, what: String, fix: &str| Row { kind, id: id.into(), tone, what, fix: fix.into(), account: false, ms: None };
    let mut rows = Vec::new();
    let mut ready: Vec<&str> = Vec::new();
    for id in DOERS {
        let d = f.doers.iter().find(|d| d.id == id).cloned().unwrap_or(DoerFact { id: id.into(), fix: format!("no connector `{id}` is declared"), ..DoerFact::default() });
        let (tone, what) = match &d.probe {
            _ if d.pending => ("dim", CHECKING.to_string()),
            None => ("bad", "not declared".to_string()),
            Some(p) if d.keyed => if p.authed { ("good", "key set".into()) } else { ("warn", "no key".into()) },
            Some(p) if !p.installed => ("bad", "not found".into()),
            Some(p) if !p.authed => ("warn", "found · not signed in".into()),
            Some(p) => ("good", match p.version.as_deref().and_then(|v| v.lines().next()).map(str::trim).filter(|v| !v.is_empty() && v.len() <= 24) {
                Some(v) => format!("found · signed in · {v}"),
                None => "found · signed in".into(),
            }),
        };
        if tone == "good" && d.work {
            ready.push(id);
        }
        rows.push(Row { account: d.account, ms: d.ms.filter(|_| d.probe.is_some() && !d.pending), ..row("doer", id, tone, what, if tone == "good" || d.pending { "" } else { &d.fix }) });
    }
    ready.sort_unstable();
    let waiting: Vec<&str> = DOERS.into_iter().filter(|id| f.doers.iter().any(|d| d.id == *id && d.pending)).collect();
    rows.push(match f.key {
        true => row("key", "openrouter key", "good", "set".into(), ""),
        false => row("key", "openrouter key", "warn", "not set".into(), &f.key_fix),
    });
    for p in &f.plans {
        rows.push(match p.price {
            Some(usd) => row("plan", &format!("plan {}", p.id), "good", format!("${usd:.0} a {}", p.per), ""),
            None => row("plan", &format!("plan {}", p.id), "warn", "no price".into(), &format!("Library › Plans: ⏎ on {} sets its price a {}", p.id, p.per)),
        });
    }
    for (id, what) in SAFE {
        rows.push(row("safe", id, "good", what.into(), ""));
    }
    // the binary makes itself not dumpable at its start on Linux, unless the switch for a debugger is set
    if cfg!(target_os = "linux") {
        let open = std::env::var_os("J4N_DUMPABLE").is_some_and(|v| v == "1");
        let what = if open { "memory dumps and other programs' reads of J4n are allowed: J4N_DUMPABLE=1 is set" } else { "memory dumps and other programs' reads of J4n are blocked (J4N_DUMPABLE=1 allows them)" };
        rows.push(row("safe", "memory", if open { "warn" } else { "good" }, what.into(), ""));
    }
    // the operator's pick wins when it is ready; else the first election's: the smallest id of the ready doers (`rank.rs`)
    let default = f.pick.as_deref().filter(|p| ready.contains(p)).or(ready.first().copied()).map(String::from);
    // while a probe is out, the default stands only where no answer can change it: the pick is ready, or every doer still out sorts after it
    let settled = match (f.pick.as_deref(), default.as_deref()) {
        _ if waiting.is_empty() => true,
        (Some(p), Some(d)) if p == d => true,
        (Some(p), _) if waiting.contains(&p) => false,
        (_, Some(d)) => waiting.iter().all(|w| *w > d),
        (_, None) => false,
    };
    let default = default.filter(|_| settled);
    let hint = match (&f.pick, &default) {
        (Some(p), Some(d)) if p == d => format!("your pick · {AGENT}: ⏎ on another uses it"),
        (Some(p), _) => format!("your pick, {p}, is not ready · {AGENT} › {p} says what mends it"),
        (None, _) if ready.len() > 1 => format!("{AGENT}: ⏎ on one uses it for the main lane"),
        _ => String::new(),
    };
    rows.push(match &default {
        Some(d) => row("default", "default doer", "good", d.clone(), &hint),
        None if !settled => row("default", "default doer", "dim", CHECKING.into(), ""),
        None => {
            // a ready doer that takes no work (`light`: a key and no tools) does not make a default, and the mend names what does
            let ids = |keep: &dyn Fn(&DoerFact) -> bool| f.doers.iter().filter(|d| keep(d)).map(|d| d.id.as_str()).collect::<Vec<_>>().join(", ");
            let only = ids(&|d| !d.work && d.probe.as_ref().is_some_and(|p| p.installed && p.authed));
            let fix = if only.is_empty() { format!("{AGENT}: ⏎ sets one up") } else { format!("{only} only answers questions and takes no work: sign in to one of {} · {AGENT}", ids(&|d| d.work)) };
            row("default", "default doer", "bad", "none works".into(), &fix)
        }
    });
    for n in &f.needs {
        rows.push(row("need", &format!("need {}", n.id), "warn", n.what.clone(), &n.fix));
    }
    // a pack that is on and is not basic says so and why: the folder uses it, an `enable` row, `packs.full`; it is not listed off
    if f.full {
        rows.push(row("kept", "packs.full", "good", "on".into(), "every built-in pack but test is loaded · Settings › Features › packs.full turns it off"));
        // what it turned on, one row for each; the ones it leaves to the operator are named once, with the reason
        for (id, v) in &f.full_sets {
            rows.push(row("kept", id, "good", v.clone(), "on because of packs.full · your own row for it wins, Settings › Features shows it"));
        }
        rows.push(row("kept", "packs.full leaves", "dim", "yours".into(), "contribute.mode, inbox.mode, kit.judge, qc.jev, reflexes.judge, librarian.on, weigh.on and the accounts: each sends text or a line out of the project or acts for you, so each stays as you set it"));
    }
    for (id, why) in &f.kept {
        rows.push(row("kept", &format!("pack {id}"), "good", "on".into(), &format!("{why} · Grow › Packs › {id}: ⏎ turns it off")));
    }
    let kept = |id: &str| f.kept.iter().any(|(k, _)| id == format!("pack {k}"));
    for (id, what) in BASIC_OFF.iter().filter(|(id, _)| !kept(&format!("pack {id}"))) {
        rows.push(row("off", &format!("pack {id}"), "dim", "off".into(), &format!("{what}; not in J4n basic · Grow › Packs › {id}: ⏎, or Settings › Features › packs.full for all")));
    }
    // a key that is set while the pack that would use it is off: said once, with the key that turns the pack on
    if f.key && !kept("pack jev") {
        rows.push(row("key", "jev key", "warn", "key set, pack off".into(), &format!("the Decider would use your OpenRouter key · {SPAN}: o turns it on for every project")));
    }
    if f.orchestrating {
        rows.push(row("kept", "lanes", "good", "orchestrate".into(), "the quick look, the crew's seats and the envoys follow the word (`auto`) · `:lanes single` turns them off"));
    }
    for (id, why, how) in OFF.iter().filter(|(id, ..)| !kept(id) && !(f.orchestrating && *id == "lanes") && !(f.full && *id == "packs.full")) {
        rows.push(row("off", id, "dim", "off".into(), &format!("{why} · {how}")));
    }
    Report { rows, default, checking: !waiting.is_empty() }
}

/// The report while nothing is known yet: each doer and the key are being checked, the defaults that hold in any folder are said.
fn unknown() -> Report {
    let mut r = report(&Facts { doers: DOERS.iter().map(|id| DoerFact { id: (*id).into(), pending: true, ..DoerFact::default() }).collect(), ..Facts::default() });
    r.rows.retain(|r| matches!(r.kind, "doer" | "key" | "safe" | "default"));
    for k in r.rows.iter_mut().filter(|r| r.kind == "key") {
        (k.tone, k.what) = ("dim", CHECKING.into());
        k.fix.clear();
    }
    r
}

/// Where a mend line leads: the Agent step of Connect › Setup, and the Decider row that takes the OpenRouter key.
pub const AGENT: &str = "Connect › Setup › Agent";
pub const SPAN: &str = "Connect › Setup › Decider › Span through OpenRouter";

/// The defaults that hold in any folder, in words.
const SAFE: [(&str, &str); 4] = [
    ("writes", "a write outside the project asks first"),
    (".j4n and git", "J4n's own folder and git's internals are blocked"),
    ("checkpoint", "a checkpoint is made before work, to rewind to"),
    ("gate", "claude's writes and commands come to the Gate first"),
];

/// The packs J4n basic leaves off, with what each does.
pub fn basic_off() -> &'static [(&'static str, &'static str)] {
    &BASIC_OFF
}

/// The packs J4n basic leaves off: `(id, what it does)`; the packs of the table below are off for a reason of their own.
const BASIC_OFF: [(&str, &str); 13] = [
    ("agent", "the own agent, a model on your gateway key, and its two loops"), ("code", "the /plan, /design and /build flows"),
    ("jev", "Jev, the deciders and the small model for the ask points"), ("contribute", "what this J4n finds wrong with J4n, kept on this machine"),
    ("inbox", "tags and receipts for lines typed while a task works"),
    ("ledger", "one report per settled thread"), ("parts", "the parts of a document, one at a time"), ("habits", "show-only detectors of waste"),
    ("kit", "the tool `py` and its kernel"), ("accounts", "several logins of one agent, kept apart"), ("skills", "J4n's own skill library"),
    ("offload", "light chores to a cheaper doer"), ("reflexes", "commands that run when your sentence holds"),
];

/// What is off when the operator has said nothing, and why: `(what, the reason it is off, how to
/// turn it on)`. A test keeps this list the same as the packs that are not in `ON` and the settings whose default is off.
const OFF: [(&str, &str, &str); 27] = [
    ("pack qc", "marks on code, `qc.on`; not measured yet", "Grow › Packs › qc: ⏎"),
    ("pack shape", "notes under your line, `shape.on`; it has not been shown that they help", "Grow › Packs › shape: ⏎"),
    ("pack bench", "runs cases that spend money; yours to start", "Grow › Packs › bench: ⏎"),
    ("pack terminal", "gives a model your tmux panes; a pack to choose", "Grow › Packs › terminal: ⏎"),
    ("pack test", "empty: the scripted test doer is in base (never for work)", "Grow › Packs › test: ⏎"),
    ("packs.full", "J4n basic is the default; J4n full loads every pack built in, to try the whole of J4n", "Settings › Features › packs.full"),
    ("connect.auto", "/connect takes the best guess instead of asking what a program is; off until the lab shows the guess is right", "Settings › All › connect.auto"),
    ("lanes", "single: auto planning did not beat it in our benchmarks; `orchestrate` is the one switch of Main, the chats, the crew and the envoys", "Settings › All › lanes"),
    ("inbox.mode", "off until the tags are measured; a line steers the task at once", "Settings › All › inbox.mode"),
    ("parts.mode", "commits the parts of a document; yours to start", "Settings › All › parts.mode"),
    ("kit.mode", "Python for the own agent showed no gain on the real agent", "Settings › All › kit.mode"),
    ("kit.judge", "sends text to Jev; per project, yours to allow", "Settings › All › kit.judge"),
    ("route.hosts", "the host pick waits for more measured runs", "Settings › All › route.hosts"),
    ("watch.upstream", "asks the internet, quietly and on a timer, whether the tools J4n uses have a new release; off until you want it", "Settings › All › watch.upstream"),
    ("watch.canary", "installs and runs a new release once; yours to allow", "Settings › All › watch.canary"),
    ("web.local", "a fetch may not reach your machine or network", "Settings › Features › web.local"),
    ("brief.recall", "off until a measured run shows fewer turns and no drop in pass", "Settings › All › brief.recall"),
    ("brief.plain_answer", "off until an A/B run shows no drop in pass", "Settings › All › brief.plain_answer"),
    ("retry.auto", "acts by itself on a failed turn; off until it is checked on real work", "Settings › Features › retry.auto"),
    ("read.big_heads", "only kitty 0.40 and later draws it", "Settings › All › read.big_heads"),
    ("weigh.on", "routes a line by its weight (a ping to a cheap model); off until it is measured on a drill of mixed lines", "Settings › All › weigh.on"),
    ("librarian.on", "a task's context is made for the task and the doer; off until it is measured against the fixed context", "Settings › All › librarian.on"),
    ("reflexes.on", "commands that run by themselves when your sentence holds, judged by Jev; off until a reflex's precision is measured", "Settings › All › reflexes.on"),
    ("reflexes.judge", "sends the digest of a chat to Jev; per project, yours to allow", "Settings › All › reflexes.judge"),
    ("chats.on", "every prompt can be a chat of its own; off until it has been used on a real project", "Settings › All › chats.on"),
    ("qc.jev", "sends the code of a change to Jev; yours to allow", "Settings › All › qc.jev"),
    ("qc.provenance", "changes what Jev is sent; off until measured", "Settings › All › qc.provenance"),
];

impl Row {
    /// What the row says, with the time its probe took when it was measured and the doer is ready (`found · signed in · 2.1.5 · 85 ms`): one that needs
    /// mending says the mend, and its time is not what starting it costs.
    pub fn shown(&self) -> String {
        match self.ms.filter(|_| self.fix.is_empty()) {
            Some(ms) => format!("{} · {ms} ms", self.what),
            None => self.what.clone(),
        }
    }
}

impl Report {
    /// 0 when the default doer works, 1 when none does.
    pub fn code(&self) -> i32 {
        i32::from(self.default.is_none())
    }

    /// The report as `j4n doctor` prints it: a line a row, a mend after a dash.
    pub fn text(&self) -> String {
        let lines = self.rows.iter().map(|r| format!("{:<7} {:<14} {}{}", r.kind, r.id, r.shown(), if r.fix.is_empty() { String::new() } else { format!(" — {}", r.fix) }));
        lines.collect::<Vec<_>>().join("\n") + "\n"
    }

    /// The one row a new folder's chat shows: what is not found, off (its pack is off), not signed in or without a key, and where to mend it. None: all is fine.
    pub fn notice(&self) -> Option<String> {
        let who = |what: &dyn Fn(&Row) -> bool| self.rows.iter().filter(|r| r.kind == "doer" && what(r)).map(|r| r.id.as_str()).collect::<Vec<_>>().join(", ");
        let parts: Vec<String> = [(who(&|r| r.what == "not found"), "not found"), (who(&|r| r.what == "not declared"), "off"), (who(&|r| r.what.contains("not signed in")), "not signed in"), (who(&|r| r.what == "no key"), "")]
            .into_iter()
            .filter(|(ids, _)| !ids.is_empty())
            .map(|(ids, what)| if what.is_empty() { format!("no key for {ids}") } else { format!("{ids} {what}") })
            .collect();
        // `⏎` opens Connect › Setup, whose rows mend each of them: a program, its sign-in, a key, a pack that is off
        (!parts.is_empty()).then(|| format!("{} · ⏎ opens Connect › Setup", parts.join("; ")))
    }

    /// The doctor's rows of `fold:setup`, which `view:setup` lists (after the steps of Connect › Setup, which it leaves out).
    pub fn values(&self) -> Vec<Value> {
        self.rows
            .iter()
            .map(|r| {
                let text = |t: &str| Value::text(t);
                Value::Map(BTreeMap::from([
                    ("id".into(), text(&r.id)),
                    ("kind".into(), text(r.kind)),
                    ("tone".into(), text(r.tone)),
                    ("what".into(), text(&r.shown())),
                    ("fix".into(), text(&r.fix)),
                    ("account".into(), text(if r.account { "yes" } else { "" })),
                ]))
            })
            .collect()
    }
}

/// The pre-flight as the front holds it. The screen never waits for a probe: [`Check::report_for`] says the last facts at once and, when there
/// are none or they are stale, starts one look on a thread of its own (never two at once). Each doer's answer lands as it comes and the facts
/// are stamped when the look ends, so a slow look is not stale when it lands; [`Check::landed`] counts each landing, for the screen to draw
/// again. A program that was not there, or did not answer in its probe's time, keeps that answer for [`BACKOFF_MS`], unless Setup's `r` asks
/// ([`Check::again`]): a launcher that installs on its first call is not started again every few seconds.
#[derive(Clone)]
pub struct Check {
    /// What is known without running a program: the doers declared (each `pending`), the key, the plans, the packs.
    frame: Arc<dyn Fn() -> Facts + Send + Sync>,
    /// One doer's own probe, which takes as long as the program takes.
    probe: Arc<dyn Fn(&str) -> DoerFact + Send + Sync>,
    clock: Arc<dyn Clock>,
    held: Arc<Held>,
}

/// How long the facts stand, from the end of the look that found them, before the doers are asked again.
pub const STALE_MS: u64 = 10_000;
/// How long a program that was not there, or did not answer in time, keeps that answer before it is asked again.
pub const BACKOFF_MS: u64 = 5 * 60_000;

#[derive(Default)]
struct Held {
    state: Mutex<State>,
    ended: Condvar,
    landed: AtomicU64,
}

#[derive(Default)]
struct State {
    /// The facts of the last look that ended: when it ended, and the round it was started in.
    last: Option<(u64, u64, Facts)>,
    /// The look that runs, as its answers land.
    part: Option<Facts>,
    busy: bool,
    /// Moved by [`Check::again`]: the facts of an earlier round are not fresh.
    round: u64,
    /// How many looks have ended.
    ended: u64,
    /// The doers that keep their last answer, and until when.
    slow: BTreeMap<String, u64>,
    /// A look that found nothing, with none before it: when it ended, and its round.
    failed: Option<(u64, u64)>,
}

impl State {
    fn fresh(&self, now: u64) -> bool {
        let stands = |at: u64, round: u64| round == self.round && now.saturating_sub(at) < STALE_MS;
        self.last.as_ref().is_some_and(|(at, round, _)| stands(*at, *round)) || self.failed.is_some_and(|(at, round)| stands(at, round))
    }

    /// What a report says now: the look that runs, with its doers not answered yet as the last look found them; else the last look.
    fn shown(&self) -> Option<Facts> {
        let last = self.last.as_ref().map(|(.., f)| f);
        let Some(p) = &self.part else { return last.cloned() };
        let was = |id: &str| last.and_then(|l| l.doers.iter().find(|d| d.id == id && !d.pending)).cloned();
        Some(Facts { doers: p.doers.iter().map(|d| if d.pending { was(&d.id).unwrap_or_else(|| d.clone()) } else { d.clone() }).collect(), ..p.clone() })
    }
}

/// The look ends however it ends, a panic too: what it found is stamped now, it is no longer busy, and whoever waits for it is woken. A look
/// that found nothing (its frame failed) stamps the last facts again, or none, so the next is not started at once, at every draw.
struct Ends {
    check: Check,
    round: u64,
}

impl Drop for Ends {
    fn drop(&mut self) {
        let (held, now) = (&self.check.held, self.check.clock.now_ms());
        let mut s = lock(&held.state);
        match s.part.take() {
            Some(found) => (s.last, s.failed) = (Some((now, self.round, found)), None),
            None => match s.last.take() {
                Some((.., f)) => s.last = Some((now, self.round, f)),
                None => s.failed = Some((now, self.round)),
            },
        }
        s.busy = false;
        s.ended += 1;
        drop(s);
        held.landed.fetch_add(1, Ordering::SeqCst);
        held.ended.notify_all();
    }
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Check {
    pub fn new(frame: impl Fn() -> Facts + Send + Sync + 'static, probe: impl Fn(&str) -> DoerFact + Send + Sync + 'static) -> Check {
        Check { frame: Arc::new(frame), probe: Arc::new(probe), clock: Arc::new(SystemClock), held: Arc::default() }
    }

    /// The clock the stamps and the waits read (a test's moves by hand).
    pub fn with_clock(self, clock: Arc<dyn Clock>) -> Check {
        Check { clock, ..self }
    }

    /// The doers are asked again at the next report, one that was not there too: a program was installed or signed in (Connect › Setup `r`). What was
    /// found stands until the new answers land.
    pub fn again(&self) {
        let mut s = lock(&self.held.state);
        s.round += 1;
        s.slow.clear();
    }

    /// How many answers have landed: a screen draws the views again when it moved.
    pub fn landed(&self) -> u64 {
        self.held.landed.load(Ordering::SeqCst)
    }

    /// The report as it stands, at once, with the main lane's pick as the operator last said it (`:setup doer`), which the facts do not hold.
    /// A doer whose probe has not answered says `checking…`; when the facts are missing or stale a look is started, and nothing waits for it.
    pub fn report_for(&self, pick: Option<String>) -> Report {
        let mut s = lock(&self.held.state);
        if !s.busy && !s.fresh(self.clock.now_ms()) {
            self.start(&mut s);
        }
        let shown = s.shown();
        drop(s);
        Self::said(shown, pick)
    }

    /// The report on fresh facts, waiting for the look that runs or for one it starts: for a caller that acts on what it says (the Setup
    /// card, the note of a new folder, Setup's test), never for the screen's own work.
    pub fn report_now(&self, pick: Option<String>) -> Report {
        let mut s = lock(&self.held.state);
        // a look started before `again` is not the one asked for: the next one is
        for _ in 0..2 {
            if s.fresh(self.clock.now_ms()) {
                break;
            }
            if !s.busy {
                self.start(&mut s);
            }
            let seen = s.ended;
            while s.ended == seen {
                s = self.held.ended.wait(s).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
        let shown = s.shown();
        drop(s);
        Self::said(shown, pick)
    }

    fn said(facts: Option<Facts>, pick: Option<String>) -> Report {
        match facts {
            Some(f) => report(&Facts { pick: pick.or(f.pick.clone()), ..f }),
            None => unknown(),
        }
    }

    /// One look, on a thread of its own: the frame lands first, then each doer's answer as it comes, each probe on a thread of its own.
    fn start(&self, s: &mut State) {
        s.busy = true;
        let (round, me) = (s.round, self.clone());
        std::thread::spawn(move || {
            let ends = Ends { check: me.clone(), round };
            let mut frame = (me.frame)();
            let now = me.clock.now_ms();
            {
                let mut s = lock(&me.held.state);
                s.slow.retain(|_, until| *until > now);
                let s = &mut *s;
                let last = s.last.as_ref().map(|(.., f)| f);
                // a doer that keeps its last answer is not asked
                for d in frame.doers.iter_mut().filter(|d| d.pending && s.slow.contains_key(&d.id)) {
                    if let Some(old) = last.and_then(|l| l.doers.iter().find(|o| o.id == d.id && !o.pending)) {
                        *d = old.clone();
                    }
                }
                s.part = Some(frame.clone());
            }
            me.held.landed.fetch_add(1, Ordering::SeqCst);
            let due: Vec<String> = frame.doers.iter().filter(|d| d.pending).map(|d| d.id.clone()).collect();
            std::thread::scope(|scope| {
                for id in &due {
                    let me = &me;
                    scope.spawn(move || me.probe_one(id, round));
                }
            });
            drop(ends);
        });
    }

    /// One doer's probe, landed in the look that runs. A program that was not there, or did not answer in its probe's time (its probe then
    /// finds none), keeps this answer for [`BACKOFF_MS`]; a key on a gateway runs no program and is asked each time. An `again` while the
    /// probe ran is the operator's: this answer arms no wait.
    fn probe_one(&self, id: &str, round: u64) {
        // a probe that panics found nothing there
        let fact = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.probe)(id)))
            .unwrap_or_else(|_| DoerFact { id: id.into(), probe: Some(Probe::default()), fix: format!("its probe failed · {AGENT} › {id}: r looks again"), ..DoerFact::default() });
        let now = self.clock.now_ms();
        let mut s = lock(&self.held.state);
        if !fact.keyed && fact.probe.as_ref().is_some_and(|p| !p.installed) {
            if s.round == round {
                s.slow.insert(id.into(), now + BACKOFF_MS);
            }
        } else {
            s.slow.remove(id);
        }
        if let Some(d) = s.part.as_mut().and_then(|p| p.doers.iter_mut().find(|d| d.id == id)) {
            *d = DoerFact { pending: false, ..fact };
        }
        drop(s);
        self.held.landed.fetch_add(1, Ordering::SeqCst);
    }
}
