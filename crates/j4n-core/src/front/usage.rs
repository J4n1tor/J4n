//! `/usage`: where each subscription's windows and each gateway's spend stand, from the Log and nothing else. No model,
//! no doer, no network. A reading is the last `run.window` of a doer, an account and a window (`utilization` 0 to 1, `resets_at` in
//! seconds); the pace is the use over the time since the window began (its reset less its length, which the plan row knows); the view is
//! a list of entries for the screen's `meter` (a group, a bar with its note, a line of words) held in a chat row (`chat.view`).

use super::Front;
use crate::change::Actor;
use crate::cost::Plans;
use crate::log::{Ev, Log};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The id of this view in a `chat.view` event and in the row that holds it.
pub const VIEW: &str = "usage";
/// The gateway whose key says what it has left (`credits`), and the doers that report no window as yet.
const GATEWAY: &str = "openrouter";
const SILENT: [&str; 2] = ["codex", "gemini"];
const DAY_MS: u64 = 86_400_000;
const HOUR_MS: f64 = 3_600_000.0;
/// How many of the newest events the first read goes through.
const KEEP: u64 = 100_000;
/// A model call whose price was not said counts as this much (the judge of a reflex).
pub(super) const UNKNOWN_USD: f64 = 0.01;
/// A window younger than this has no pace yet: one step of a percent in a few minutes says nothing.
const YOUNG_MS: u64 = 5 * 60_000;
/// A reading older than this is said to be.
const OLD_MS: u64 = 10 * 60_000;

/// What a gateway says is left of a key's credits: `remaining` of the limit the key has (none: it has no limit), `used` in all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Left {
    pub remaining: Option<f64>,
    pub used: f64,
    pub limit: Option<f64>,
}

/// How long a reading of a gateway's credits stands before it is asked for again: the call is made at most this often.
pub const CREDITS_EVERY_MS: u64 = 10 * 60_000;

/// The credits of a key as the front keeps them: `ask` is the one network call; [`Credits::left`] makes it only when the last answer (even a
/// failed one) is [`CREDITS_EVERY_MS`] old, so a screen drawn again or a chore that asks does not call the gateway again.
#[derive(Clone)]
pub struct Credits {
    ask: Arc<dyn Fn() -> Result<Left, String> + Send + Sync>,
    last: Arc<Mutex<Last>>,
}

/// When the gateway was last asked, and what it said.
type Last = Option<(u64, Result<Left, String>)>;

impl Credits {
    pub fn new(ask: impl Fn() -> Result<Left, String> + Send + Sync + 'static) -> Credits {
        Credits { ask: Arc::new(ask), last: Arc::default() }
    }

    /// What the key has left at `now_ms`: the last answer while it is fresh, else the gateway is asked.
    pub fn left(&self, now_ms: u64) -> Result<Left, String> {
        let mut last = self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, got)) = last.as_ref().filter(|(at, _)| now_ms.saturating_sub(*at) < CREDITS_EVERY_MS) {
            return got.clone();
        }
        let got = (self.ask)();
        *last = Some((now_ms, got.clone()));
        got
    }

    /// The last answer, with no call: none before the first.
    pub fn last(&self) -> Option<Result<Left, String>> {
        self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().map(|(_, g)| g.clone())
    }
}

/// What a window of a doer, run as an account, said last.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub util: f64,
    pub resets_s: i64,
    pub at_ms: u64,
}

/// Where a login stands after its last sign-in or look (`account.signin`, `account.checked`): whether it is signed in, and the plan word the
/// harness gave (`max`, `pro`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signed {
    pub ok: bool,
    pub plan: String,
}

/// What the Log says that `/usage` is made of: the readings, and the dollars each doer spent each day (and each ask point's model calls, under `#point`).
#[derive(Default)]
pub struct Usage {
    loaded: bool,
    /// `/usage` was asked for in this run: only then is the gateway asked what a key has left.
    asked: bool,
    last: u64,
    windows: BTreeMap<(String, String, String), Reading>,
    spent: BTreeMap<(String, u64), (f64, u32)>,
    /// The windows that said `rejected` and have not said `allowed` since, as the epoch (ms) they reset at.
    blocked: BTreeMap<(String, String, String), u64>,
    /// A limit a run said in words with the time it lasts (`run.error`, class `rate-limit`): the event's number and when it ends (ms). A reading
    /// of the same login that comes after it takes it back: the login works again.
    waits: BTreeMap<(String, String), (u64, u64, String)>,
    /// Each login of this project, as its last sign-in or look left it, by account name.
    signed: BTreeMap<String, Signed>,
}

impl Usage {
    /// Read the Log once, the first time a view is filled.
    pub fn load(&mut self, log: &Log) {
        if !std::mem::replace(&mut self.loaded, true) {
            log.read(log.head().saturating_sub(KEEP)).for_each(|e| self.count(&e));
        }
    }

    /// An event as the server is told of it: counted once the Log has been read.
    pub fn see(&mut self, ev: &Ev) {
        if self.loaded && ev.seq > self.last {
            self.count(ev);
        }
    }

    /// How full the tightest window of a doer, run as an account, stands at `now_ms` (0 to 1), from the last reading of each; a window that has
    /// reset since its reading is not counted. None: no window of it is known (a skill's pick reads this to call a window busy).
    pub fn tightest(&self, doer: &str, account: &str, now_ms: u64) -> Option<f64> {
        let live = |r: &&Reading| u64::try_from(r.resets_s).unwrap_or(0).saturating_mul(1000) > now_ms;
        self.windows.iter().filter(|((d, a, _), _)| d == doer && a == account).map(|(_, r)| r).filter(live).map(|r| r.util).reduce(f64::max)
    }

    /// When the login (`account` empty: the doer's own) is let in again for `model` (empty: not said), if a window of it is shut now: the latest of the
    /// windows that said `rejected` and have not reset, or of a limit said in words that no later reading of that login has taken back. A window
    /// of one family (`seven_day_opus`) shuts the models of that family only, and a limit said by a run of one model shuts that model only.
    /// None: it is open as far as the Log knows.
    pub fn limited_until(&self, doer: &str, account: &str, model: &str, now_ms: u64) -> Option<u64> {
        let model = model.to_lowercase();
        fn family(w: &str) -> Option<&str> {
            w.rsplit_once('_').map(|(_, f)| f).filter(|f| matches!(*f, "opus" | "sonnet" | "haiku"))
        }
        let shut = self.blocked.iter().filter(|((d, a, w), _)| d == doer && a == account && family(w).is_none_or(|f| model.contains(f))).map(|(_, at)| *at);
        let said = self.waits.get(&(doer.to_string(), account.to_string())).filter(|(_, _, m)| m.is_empty() || model.is_empty() || model.contains(m.as_str()) || m.contains(model.as_str())).map(|(_, until, _)| *until);
        shut.chain(said).filter(|at| *at > now_ms).max()
    }

    /// Where the login `account` stands as of its last sign-in or look; none: nothing was ever learned of it.
    pub fn signed(&self, account: &str) -> Option<&Signed> {
        self.signed.get(account)
    }

    /// Count what the Log holds that was written since the last event counted: the readings stand as of now, in a front no screen tells of events.
    pub fn catch_up(&mut self, log: &Log) {
        self.load(log);
        log.read(self.last + 1).for_each(|e| self.count(&e));
    }

    pub fn count(&mut self, ev: &Ev) {
        self.last = self.last.max(ev.seq);
        let d = &ev.data;
        // a model step of an ask says what it cost (`null`: not said); the ask id begins with its point
        if let (true, Some(usd), Some(ask)) = (ev.kind == "ask.step", d.get("usd"), d.get("ask").and_then(Json::as_str)) {
            let e = self.spent.entry((format!("#{}", ask.split('#').next().unwrap_or_default()), ev.at_ms / DAY_MS)).or_default();
            *e = (e.0 + usd.as_f64().unwrap_or(UNKNOWN_USD), e.1 + 1);
        }
        let account = d.get("account").and_then(Json::as_str).unwrap_or_default().to_string();
        match ev.kind.as_str() {
            // what a sign-in or a look learned of a login: signed in, or not, and its plan (a removed one is neither)
            "account.signin" | "account.checked" | "account.removed" => {
                let ok = ev.kind != "account.removed" && d.get("ok").and_then(Json::as_bool).unwrap_or(false);
                let said = d.get("plan").and_then(Json::as_str).map(str::to_string);
                let was = self.signed.remove(&account).filter(|_| ok).unwrap_or_default();
                self.signed.insert(account, Signed { ok, plan: said.unwrap_or(was.plan) });
                return;
            }
            _ => {}
        }
        let Actor::Doer(doer) = &ev.by else { return };
        match ev.kind.as_str() {
            "run.window" => {
                if let (Some(w), Some(util), Some(resets_s)) = (d.get("window").and_then(Json::as_str), d.get("utilization").and_then(Json::as_f64), d.get("resets_at").and_then(Json::as_i64)) {
                    let key = (doer.clone(), account.clone(), w.to_string());
                    let shut = d.get("status").and_then(Json::as_str).is_some_and(|s| s.starts_with("rejected") || s == "blocked");
                    match shut {
                        true => drop(self.blocked.insert(key.clone(), u64::try_from(resets_s).unwrap_or(0).saturating_mul(1000))),
                        false => drop(self.blocked.remove(&key)),
                    }
                    // a reading after a limit said in words takes it back
                    if self.waits.get(&(doer.clone(), account.clone())).is_some_and(|(seq, _, _)| *seq < ev.seq) && !shut {
                        self.waits.remove(&(doer.clone(), account));
                    }
                    self.windows.insert(key, Reading { util, resets_s, at_ms: ev.at_ms });
                }
            }
            "run.error" => {
                if let (Some("rate-limit"), Some("limit"), Some(wait)) = (d.get("class").and_then(Json::as_str), d.get("row").and_then(Json::as_str), d.get("wait_ms").and_then(Json::as_u64)) {
                    let model = d.get("model").and_then(Json::as_str).unwrap_or_default().to_lowercase();
                    self.waits.insert((doer.clone(), account), (ev.seq, ev.at_ms.saturating_add(wait), model));
                }
            }
            "run.metrics" => {
                let e = self.spent.entry((doer.clone(), ev.at_ms / DAY_MS)).or_default();
                *e = (e.0 + d.get("usd").and_then(Json::as_f64).unwrap_or(0.0), e.1 + 1);
            }
            _ => {}
        }
    }
}

/// How a window is going to end, at the pace it has had.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    /// Too young a window to have a pace.
    Unknown,
    /// Nothing used.
    Idle,
    /// All of it is used.
    Full,
    /// Milliseconds from now to the end of it.
    In(u64),
    /// It lasts past the reset.
    PastReset,
}

/// A window as a bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub used: f64,
    /// Milliseconds to the reset; none: it has reset since the reading, and the next run opens a new window.
    pub reset_in: Option<u64>,
    /// Use per hour, as a fraction of the window.
    pub per_hour: Option<f64>,
    pub end: End,
}

/// A reading of a window `len_ms` long, as it stands at `now_ms`. The pace is the use over the time from the window's start to the reading.
pub fn bar(r: &Reading, len_ms: u64, now_ms: u64) -> Bar {
    let resets = u64::try_from(r.resets_s).unwrap_or(0).saturating_mul(1000);
    if now_ms >= resets {
        return Bar { used: 0.0, reset_in: None, per_hour: None, end: End::Idle };
    }
    let (used, reset_in) = (r.util.max(0.0), Some(resets - now_ms));
    let elapsed = r.at_ms.max(resets.saturating_sub(len_ms)) - resets.saturating_sub(len_ms);
    if used <= 0.0 {
        return Bar { used, reset_in, per_hour: None, end: End::Idle };
    }
    if elapsed < YOUNG_MS {
        return Bar { used, reset_in, per_hour: None, end: if used >= 1.0 { End::Full } else { End::Unknown } };
    }
    let rate = used / elapsed as f64;
    let end = match r.at_ms as f64 + (1.0 - used).max(0.0) / rate {
        _ if used >= 1.0 => End::Full,
        at if at >= resets as f64 => End::PastReset,
        at => End::In((at as u64).saturating_sub(now_ms)),
    };
    Bar { used, reset_in, per_hour: Some(rate * HOUR_MS), end }
}

/// `1:40`, or `3d 4h` from two days.
pub fn hm(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 2 * 1440 { format!("{}d {}h", m / 1440, m % 1440 / 60) } else { format!("{}:{:02}", m / 60, m % 60) }
}

/// `5h`, `7d`: the length of a window.
fn length(ms: u64) -> String {
    if ms.is_multiple_of(DAY_MS) { format!("{}d", ms / DAY_MS) } else { format!("{}h", ms / 3_600_000) }
}

/// What stands beside a bar: when it resets, how fast it goes and how it ends.
fn note(b: &Bar, r: &Reading, len_ms: u64, now_ms: u64) -> String {
    let mut parts = Vec::new();
    match b.reset_in {
        None => parts.push("reset: the next run opens a new window".to_string()),
        Some(left) if len_ms <= DAY_MS => parts.push(format!("resets in {} ({} UTC)", hm(left), hm((now_ms + left) % DAY_MS))),
        Some(left) => parts.push(format!("resets in {}", hm(left))),
    }
    if let Some(p) = b.per_hour {
        parts.push(if len_ms <= DAY_MS { format!("{:.0}%/h", p * 100.0) } else { format!("{:.0}%/day", p * 2400.0) });
    }
    match b.end {
        End::Idle if b.reset_in.is_some() => parts.push("unused".into()),
        End::Full => parts.push("full".into()),
        End::In(ms) => parts.push(format!("at this pace ends in {}", hm(ms))),
        End::PastReset => parts.push("lasts past reset".into()),
        _ => {}
    }
    if b.reset_in.is_some() && now_ms.saturating_sub(r.at_ms) > OLD_MS {
        parts.push(format!("read {} ago", hm(now_ms - r.at_ms)));
    }
    parts.join(" · ")
}

/// The entries of the view: for each doer and account that showed a window, a group and a bar for each window; a doer whose plan has windows
/// and showed none says so; each gateway says its credits (not known: they need a call) and what it was spent today.
pub fn view(u: &Usage, plans: &Plans, gateways: &[(String, String)], now_ms: u64) -> Json {
    view_with(u, plans, gateways, &[], &[], &BTreeMap::new(), now_ms)
}

/// [`view`], with the doers that report no window of any kind (one line says so), and what each gateway says it has left (a gateway not in
/// `credits` says its credits are not known: no call was made). `accounts` are the logins there are, `(doer, account)`: each one of a doer that
/// shows windows is a group, and one that has shown none says so, whatever another account has shown; with accounts, the doer's own
/// login (no account) is a group too.
pub fn view_with(u: &Usage, plans: &Plans, gateways: &[(String, String)], silent: &[String], accounts: &[(String, String)], credits: &BTreeMap<String, Result<Left, String>>, now_ms: u64) -> Json {
    let mut out = Vec::new();
    let mut groups: Vec<(&str, &str)> = u.windows.keys().map(|(d, a, _)| (d.as_str(), a.as_str())).collect();
    for doer in plans.windowed() {
        let named: Vec<&str> = accounts.iter().filter(|(d, _)| d == doer).map(|(_, a)| a.as_str()).collect();
        if !named.is_empty() {
            groups.push((doer, ""));
            groups.extend(named.into_iter().map(|a| (doer, a)));
        }
    }
    groups.sort_unstable();
    groups.dedup();
    for doer in plans.windowed().into_iter().filter(|d| !groups.iter().any(|g| g.0 == *d)) {
        out.push(json!({ "group": doer }));
        out.push(json!({ "text": "no runs yet", "tone": "dim" }));
    }
    for (doer, account) in groups {
        let plan = plans.of(doer, Some(account).filter(|a| !a.is_empty()));
        let tag = plan.filter(|p| p.id != doer).map(|p| format!(" · {}{}", p.id, p.price.map(|m| format!(" ${m:.0}/mo")).unwrap_or_default())).unwrap_or_default();
        // a login with no plan row says the plan its sign-in gave, and one that is shut says until when
        let sign = Some(account).filter(|a| !a.is_empty()).map(|a| u.signed(a).cloned().unwrap_or_default());
        let tag = match sign.as_ref().filter(|s| tag.is_empty() && s.ok && !s.plan.is_empty()) {
            Some(s) => format!(" · {}", s.plan),
            None => tag,
        };
        let shut = u.limited_until(doer, account, "", now_ms).map(|at| format!(" · limited, back in {}", hm(at - now_ms))).unwrap_or_default();
        out.push(json!({ "group": format!("{doer} · {}{tag}{shut}", if account.is_empty() { "default" } else { account }) }));
        if !u.windows.keys().any(|(d, a, _)| d == doer && a == account) {
            // no run asked it yet: what the sign-in said is all there is, and nothing is sent to ask for more
            let text = match &sign {
                Some(s) if !s.ok => "needs sign-in · no runs yet",
                Some(_) => "signed in · no runs yet",
                None => "no runs yet",
            };
            out.push(json!({ "text": text, "tone": "dim" }));
        }
        let mut rows: Vec<_> = u.windows.iter().filter(|((d, a, _), _)| d == doer && a == account).map(|((_, _, w), r)| (plan.and_then(|p| p.window_ms(w)), w, r)).collect();
        rows.sort_by_key(|(len, w, _)| (len.unwrap_or(u64::MAX), (*w).clone()));
        for (len, w, r) in rows {
            let Some(len) = len else {
                out.push(json!({ "label": w, "f": r.util, "note": "the plan row does not say how long this window is" }));
                continue;
            };
            let b = bar(r, len, now_ms);
            let tone = match b.end {
                End::Full => "bad",
                End::In(_) => "warn",
                _ => "",
            };
            // the screen counts the reset down by itself: `resets_ms` is when, `left` the words `note` has for it now
            let mut entry = json!({ "label": length(len), "f": b.used, "tone": tone, "note": note(&b, r, len, now_ms) });
            if let Some(left) = b.reset_in {
                (entry["resets_ms"], entry["left"]) = (json!(now_ms + left), json!(hm(left)));
            }
            out.push(entry);
        }
    }
    if !silent.is_empty() {
        out.push(json!({ "group": silent.join(" · ") }));
        out.push(json!({ "text": "these doers report no window: nothing to show", "tone": "dim" }));
    }
    let mut labels: Vec<&str> = gateways.iter().map(|g| g.1.as_str()).collect();
    labels.sort_unstable();
    labels.dedup();
    for label in labels {
        let today = gateways.iter().filter(|g| g.1 == label).filter_map(|g| u.spent.get(&(g.0.clone(), now_ms / DAY_MS))).fold((0.0, 0), |a, s| (a.0 + s.0, a.1 + s.1));
        out.push(json!({ "group": label }));
        out.push(match credits.get(label) {
            Some(Ok(Left { remaining: Some(r), limit, .. })) => json!({ "text": format!("credits: ${r:.2} left{}", limit.map(|l| format!(" of ${l:.2}")).unwrap_or_default()) }),
            Some(Ok(Left { remaining: None, used, .. })) => json!({ "text": format!("credits: no limit on this key · ${used:.2} used in all") }),
            Some(Err(why)) => json!({ "text": format!("credits: unknown ({why})"), "tone": "dim" }),
            None => json!({ "text": "credits: unknown (a gateway call, not made here)", "tone": "dim" }),
        });
        out.push(json!({ "text": if today.1 == 0 { "today: nothing spent".to_string() } else { format!("today ${:.2} · {} calls (UTC day)", today.0, today.1) } }));
    }
    Json::Array(out)
}

/// The view as plain lines, for a stream that cannot draw it (`chat.view` keeps them as its `text`).
pub fn plain(entries: &Json) -> String {
    let lines = entries.as_array().into_iter().flatten().map(|e| {
        let text = |k: &str| e.get(k).and_then(Json::as_str).unwrap_or_default();
        match e.get("f").and_then(Json::as_f64) {
            Some(f) => format!("  {:<3} [{:-<10}] {:>3.0}%  {}", text("label"), "#".repeat((f.clamp(0.0, 1.0) * 10.0).round() as usize), f * 100.0, text("note")),
            None if e.get("group").is_some() => text("group").to_string(),
            None => format!("  {}", text("text")),
        }
    });
    lines.collect::<Vec<_>>().join("\n")
}

impl Front {
    pub(super) fn usage(&self) -> std::sync::MutexGuard<'_, Usage> {
        self.0.usage.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// An event the server is told of: the windows and the spend it says are counted once the Log has been read.
    pub fn usage_see(&self, ev: &Ev) {
        if matches!(ev.kind.as_str(), "run.window" | "run.metrics" | "run.error" | "account.signin" | "account.checked" | "account.removed") {
            self.usage().see(ev);
        }
    }

    /// The view `id` as it stands now, `{field, value, props?}` for the screen's field of that kind (`usage` is a `meter` of entries); `None`
    /// for a view that is not known. A chart put in the chat is another id with a `line` or `bars` here.
    pub fn view_value(&self, id: &str) -> Option<Json> {
        if id != VIEW {
            return None;
        }
        let now = self.log().clock().now_ms();
        // the gateway is asked what a key has left only when `/usage` was said (the call is made outside the lock: it can take seconds)
        let asked = self.usage().asked;
        let accounts = self.accounts_now();
        let credits: BTreeMap<String, Result<Left, String>> = self.0.cfg().credits.iter().filter(|_| self.0.cfg().gateways.iter().any(|g| g.1 == GATEWAY)).filter_map(|c| Some((GATEWAY.to_string(), if asked { c.left(now) } else { c.last()? }))).collect();
        let mut u = self.usage();
        // what the Log holds now, a run with no screen included: the screen's event path counts the same events once
        u.catch_up(self.log());
        let silent: Vec<String> = SILENT.iter().filter(|d| self.0.doers.iter().any(|x| x.id() == **d) && !u.windows.keys().any(|(w, _, _)| w == **d)).map(|d| d.to_string()).collect();
        Some(json!({ "field": "meter", "value": view_with(&u, &self.0.tasks.plans(), &self.0.cfg().gateways, &silent, &accounts, &credits, now) }))
    }

    /// The logins there are, `(doer, account)`, as the accounts book says now (a row accepted a moment ago is in it).
    pub fn accounts_now(&self) -> Vec<(String, String)> {
        self.0.cfg().accounts.as_ref().map(|f| f()).unwrap_or_default()
    }

    /// How full the tightest window of `doer`, run as `account`, stands now (0 to 1), from the readings in the Log; none: it shows no window.
    pub fn window_now(&self, doer: &str, account: &str) -> Option<f64> {
        let mut u = self.usage();
        u.catch_up(self.log());
        u.tightest(doer, account, self.log().clock().now_ms())
    }

    /// What `source` took today (UTC), in dollars: `chore`, the chores paid per request, from their rows (a running one at its soft cap; one that ran on a
    /// window is not in it); else the ask point of that name, from the cost of its model calls in the Log, so a restart keeps it.
    pub fn spent_today(&self, source: &str) -> f64 {
        let today = self.log().clock().now_ms() / DAY_MS;
        if source != "chore" {
            let mut u = self.usage();
            u.catch_up(self.log());
            return u.spent.get(&(format!("#{source}"), today)).map_or(0.0, |s| s.0);
        }
        let rows = self.0.tasks.book().rows();
        let chores = rows.iter().filter(|r| r.spec.role.as_deref() == Some("chore") && r.spec.doer != "check" && r.started_ms.unwrap_or(r.queued_ms) / DAY_MS == today);
        chores.map(|r| if r.state.is_terminal() { if r.planned { 0.0 } else { r.spent as f64 / 1e6 } } else { r.spec.limits.usd_soft.unwrap_or(0.0) }).sum::<f64>().max(0.0)
    }

    /// `/usage`: the view, held in the chat as a row of its own, and its lines as plain words for a stream that cannot draw it.
    pub(super) fn usage_say(&self, thread: &str) {
        self.usage().asked = true;
        let text = plain(&self.view_value(VIEW).unwrap_or_default()["value"]);
        self.write(thread, "chat.view", Actor::Core, json!({ "view": VIEW, "title": "usage", "text": text }));
    }
}
