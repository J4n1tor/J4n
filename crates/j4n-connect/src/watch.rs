//! Following the tools J4n uses: is there a new release, did a repository move, what is installed here. The
//! lightest way that works: one conditional request per source (`If-None-Match`, else `If-Modified-Since`); a 304 writes nothing and
//! only counts, and an event exists for a change; feeds first (they do not touch GitHub's REST budget), REST only for a gap in a feed,
//! under a floor; at most four hosts at once and one request a second to a host; every call through [`Net`] (law 13).
//!
//! What it knows is the Log's (law 3): the fold `watch_seen` keeps each source's validators and the releases seen, `watch_pass` when
//! it last counted, so a restart asks nothing it need not. It never installs, updates or removes a tool.

use crate::canary::{self, Verdict};
use crate::digest::{self, Cadence, Item};
use crate::net::Net;
use crate::relevance::{self, Change, Class, Uses};
use crate::sets;
use j4n_core::change::Actor;
use j4n_core::fold::Folds;
use j4n_core::front::Upstream;
use j4n_core::log::{Log, New};
use j4n_core::proposal::{Draft, Proposer, Step, StepOp};
use j4n_core::table::{Kind, Problem, Tables};
use j4n_core::value::{Key, Value};
use serde::Deserialize;
use serde_json::{Value as Json, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as Atomic};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

mod more;

/// How many hosts are asked at once.
const PARALLEL: usize = 4;
/// REST requests an hour at most, without a token.
const REST_HOUR: usize = 10;
const HOUR_MS: u64 = 3_600_000;
/// The ids of a feed that are kept to tell what is new.
const KEEP_IDS: usize = 40;
/// How long a local `--version` may take.
const PROBE_WAIT: Duration = Duration::from_secs(5);
/// How long an install or a canary run may take.
const CANARY_WAIT: Duration = Duration::from_secs(120);

/// An entry of kind `source`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRow {
    pub id: String,
    pub doc: Option<String>,
    pub of: String,
    pub adapter: String,
    pub url: Option<String>,
    pub program: Option<String>,
    pub channel: Option<String>,
    pub tier: Option<String>,
    pub install: Option<Vec<String>>,
    pub canary: Option<Vec<String>>,
    pub record: Option<String>,
    pub tested: Option<String>,
    pub watch: Option<String>,
}

impl Kind for SourceRow {
    const NAME: &'static str = "source";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Hot,
    Warm,
    Cold,
}

/// What a version is: `Stable` is a release, `Pre` an alpha, beta or release candidate, `Nightly` a nightly or a tag that moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Pre,
    Nightly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    Atom,
    Npm,
    Pypi,
    Crates,
    /// A list of models (`models.dev`), a docs page as a set of lines, one issue of GitHub's REST.
    Models,
    Page,
    Issue,
    Probe,
}

#[derive(Debug, Clone)]
pub struct Source {
    pub id: String,
    pub of: String,
    pub adapter: Adapter,
    pub url: Option<String>,
    pub program: Option<String>,
    pub channel: Channel,
    pub tier: Tier,
    /// The canary: how the new release is put in a folder of its own, how its scripted run is made, the recorded good run, and the
    /// version the operator accepted as tested.
    pub install: Option<Vec<String>>,
    pub canary: Option<Vec<String>>,
    pub record: Option<String>,
    pub tested: Option<String>,
    /// What of the answer is watched: `ids:<providers>` for models, `issue:<n>` for an issue.
    pub watch: Option<String>,
}

impl Source {
    /// The sources of the Table. A row of a connector that is not declared is left out (the tool is not here to follow), and one
    /// that cannot be used is a problem.
    pub fn load(tables: &Tables) -> (Vec<Source>, Vec<Problem>) {
        let (mut out, mut problems) = (Vec::new(), Vec::new());
        for id in tables.ids(SourceRow::NAME) {
            let r = match tables.get::<SourceRow>(id) {
                Ok(r) => r,
                Err(p) => {
                    problems.push(p);
                    continue;
                }
            };
            if r.of.strip_prefix("connector:").is_some_and(|c| !tables.ids("connector").contains(&c)) {
                continue;
            }
            let adapter = match r.adapter.as_str() {
                "atom" => Adapter::Atom,
                "npm" => Adapter::Npm,
                "pypi" => Adapter::Pypi,
                "crates" => Adapter::Crates,
                "models" => Adapter::Models,
                "page" => Adapter::Page,
                "gh-rest" => Adapter::Issue,
                _ => Adapter::Probe,
            };
            let usable = if adapter == Adapter::Probe { r.program.as_ref().is_some_and(|p| !p.trim().is_empty()) } else { r.url.as_deref().is_some_and(|u| u.starts_with("https://") || u.starts_with("http://")) };
            if !usable {
                problems.push(Problem::refused(format!("source {}", r.id), if adapter == Adapter::Probe { "a `probe` names its `program`" } else { "it needs a `url` that starts with https://" }));
                continue;
            }
            let watch = r.watch.as_deref().unwrap_or_default();
            let named = match adapter {
                Adapter::Models => Some((!sets::providers(watch).is_empty(), "`models` names the providers it watches: `watch = \"ids:anthropic,openai\"`")),
                Adapter::Issue => Some((sets::issue_number(watch).is_some() && more::repo(r.url.as_deref().unwrap_or_default()).is_some(), "`gh-rest` follows one issue: `url` is the repository (https://github.com/<owner>/<repo>) and `watch = \"issue:<n>\"`")),
                _ => None,
            };
            if let Some((false, why)) = named {
                problems.push(Problem::refused(format!("source {}", r.id), why));
                continue;
            }
            let channel = match r.channel.as_deref() {
                Some("pre") => Channel::Pre,
                Some("nightly") => Channel::Nightly,
                _ => Channel::Stable,
            };
            let tier = match r.tier.as_deref() {
                Some("hot") => Tier::Hot,
                Some("cold") => Tier::Cold,
                _ => Tier::Warm,
            };
            out.push(Source { id: r.id, of: r.of, adapter, url: r.url, program: r.program, channel, tier, install: r.install, canary: r.canary, record: r.record, tested: r.tested, watch: r.watch });
        }
        (out, problems)
    }
}

// ---- what a feed says ----

/// One release as a feed says it, newest first in a list.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The tag: what makes a release the same one the next time.
    pub id: String,
    /// The tag as a version, `2.1.312`, `0.165.0-alpha.1`.
    pub version: String,
    pub link: Option<String>,
    pub published: Option<String>,
    /// The release notes as the feed gives them (HTML or Markdown), when it does.
    pub notes: String,
}

fn unxml(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("<![CDATA[").and_then(|t| t.strip_suffix("]]>")).unwrap_or(s);
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text between `<tag …>` and `</tag>` in `block`.
fn tag_text<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = block[from..].find(&open) {
        let at = from + i + open.len();
        if block[at..].starts_with(['>', ' ', '/']) {
            let start = block[at..].find('>')? + at + 1;
            if block[..start].ends_with("/>") {
                return None;
            }
            let end = block[start..].find(&format!("</{tag}>"))? + start;
            return Some(&block[start..end]);
        }
        from = at;
    }
    None
}

/// The value of `attr` of the first `<tag …>` in `block`.
fn attr<'a>(block: &'a str, tag: &str, name: &str) -> Option<&'a str> {
    let open = format!("<{tag} ");
    let head = &block[block.find(&open)?..];
    let head = &head[..head.find('>')?];
    let key = format!("{name}=\"");
    let start = head.find(&key)? + key.len();
    Some(&head[start..start + head[start..].find('"')?])
}

/// The first dotted number in a tag and what follows it: `rust-v0.164.0-alpha.1` is (`0.164.0`, `-alpha.1`). A number must start a
/// word or follow a `v`, and have a dot: `v2` and `nightly` are not versions.
fn split_version(tag: &str) -> Option<(&str, &str)> {
    let b = tag.as_bytes();
    let start = (0..b.len()).find(|&i| b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_ascii_alphanumeric() || matches!(b[i - 1], b'v' | b'V')) && tag[i..].split(|c: char| !(c.is_ascii_digit() || c == '.')).next().is_some_and(|d| d.trim_end_matches('.').contains('.')))?;
    let dotted = tag[start..].split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("").trim_end_matches('.');
    Some((dotted, &tag[start + dotted.len()..]))
}

/// Which channel a tag belongs to (prereleases, nightlies and moving tags are not versions).
pub fn channel_of(tag: &str) -> Channel {
    let low = tag.to_ascii_lowercase();
    match split_version(tag) {
        _ if low.contains("nightly") || low.contains("canary") => Channel::Nightly,
        None => Channel::Nightly,
        Some((_, rest)) if rest.trim().is_empty() => Channel::Stable,
        Some(_) => Channel::Pre,
    }
}

impl Channel {
    /// Whether a source on `self` takes a version of `of`: stable takes stable, pre takes both, nightly everything.
    fn admits(self, of: Channel) -> bool {
        matches!((self, of), (Channel::Stable, Channel::Stable) | (Channel::Pre, Channel::Stable | Channel::Pre) | (Channel::Nightly, _))
    }
}

/// The dotted numbers of two versions, compared as numbers.
pub fn compare(a: &str, b: &str) -> Ordering {
    let nums = |v: &str| split_version(v).map(|(d, _)| d.split('.').map(|n| n.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>()).unwrap_or_default();
    nums(a).cmp(&nums(b))
}

fn entry(tag: &str, link: Option<&str>, published: Option<&str>) -> Entry {
    let version = split_version(tag).map_or_else(|| tag.to_string(), |(d, rest)| format!("{d}{rest}"));
    Entry { id: tag.to_string(), version, link: link.map(str::to_string), published: published.map(str::to_string), notes: String::new() }
}

/// The entries of an Atom feed, or the items of an RSS one (PyPI's), as they are listed.
pub fn atom(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    for tag in ["entry", "item"] {
        let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
        let mut rest = text;
        while let Some(i) = rest.find(&open) {
            let Some(j) = rest[i..].find(&close) else { break };
            let block = &rest[i + open.len()..i + j];
            rest = &rest[i + j + close.len()..];
            let link = attr(block, "link", "href").map(unxml).or_else(|| tag_text(block, "link").map(unxml));
            let title = tag_text(block, "title").map(unxml).unwrap_or_default();
            let from_link = link.as_deref().and_then(|l| l.split_once("/releases/tag/")).map(|(_, t)| t.to_string());
            let from_id = tag_text(block, "id").map(unxml).and_then(|i| i.rsplit(['/', ':']).next().map(str::to_string)).filter(|t| !t.is_empty());
            let tag = from_link.or(Some(title.clone()).filter(|t| !t.is_empty())).or(from_id);
            if let Some(tag) = tag {
                let published = tag_text(block, "updated").or_else(|| tag_text(block, "pubDate")).or_else(|| tag_text(block, "published")).map(unxml);
                let notes = tag_text(block, "content").or_else(|| tag_text(block, "summary")).or_else(|| tag_text(block, "description")).map(unxml).unwrap_or_default();
                out.push(Entry { notes, ..entry(&tag, link.as_deref(), published.as_deref()) });
            }
        }
    }
    out
}

/// A release list of GitHub's REST API: `[{ "tag_name": …, "html_url": …, "published_at": … }]`, newest first.
fn rest_releases(text: &str) -> Vec<Entry> {
    let Ok(Json::Array(all)) = serde_json::from_str::<Json>(text) else { return Vec::new() };
    all.iter().filter_map(|r| Some(Entry { notes: r.get("body").and_then(Json::as_str).unwrap_or_default().into(), ..entry(r.get("tag_name")?.as_str()?, r.get("html_url").and_then(Json::as_str), r.get("published_at").and_then(Json::as_str)) })).collect()
}

/// What an adapter's answer says, newest first.
fn entries(adapter: Adapter, text: &str) -> Vec<Entry> {
    match adapter {
        Adapter::Atom | Adapter::Pypi => atom(text),
        Adapter::Npm => serde_json::from_str::<Json>(text).ok().and_then(|j| j.get("version").and_then(Json::as_str).map(|v| vec![entry(v, None, None)])).unwrap_or_default(),
        // the index lists a crate's versions oldest first, one JSON line each
        Adapter::Crates => text.lines().filter_map(|l| serde_json::from_str::<Json>(l).ok()).filter(|j| j.get("yanked") != Some(&json!(true))).filter_map(|j| j.get("vers").and_then(Json::as_str).map(|v| entry(v, None, None))).rev().collect(),
        Adapter::Models | Adapter::Page | Adapter::Issue | Adapter::Probe => Vec::new(),
    }
}

// ---- the budget ----

/// What may be asked of which host: the floor of GitHub's REST, and the hosts that said to wait.
#[derive(Default)]
pub struct Budget {
    cooling: BTreeMap<String, u64>,
    rest: VecDeque<u64>,
    remaining: Option<u64>,
}

impl Budget {
    /// GitHub said how many requests are left.
    pub fn left(&mut self, remaining: u64) {
        self.remaining = Some(remaining);
    }

    /// A REST request was made.
    pub fn used(&mut self, now: u64) {
        self.rest.push_back(now);
    }

    /// `host` was told to wait until `until`.
    pub fn cool(&mut self, host: &str, until: u64) {
        self.cooling.insert(host.into(), until);
    }

    pub fn cools(&self, host: &str, now: u64) -> bool {
        self.cooling.get(host).is_some_and(|until| *until > now)
    }

    /// A REST request is within the floor: at most [`REST_HOUR`] an hour, and a sixth of what GitHub says is left.
    pub fn rest_ok(&mut self, host: &str, now: u64) -> bool {
        while self.rest.front().is_some_and(|t| now.saturating_sub(*t) >= HOUR_MS) {
            self.rest.pop_front();
        }
        let cap = self.remaining.map_or(REST_HOUR as u64, |r| (r / 6).min(REST_HOUR as u64));
        !self.cools(host, now) && (self.rest.len() as u64) < cap
    }
}

/// The host of a URL, in lower case.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or_default().rsplit('@').next().unwrap_or_default().to_ascii_lowercase()
}

// ---- the state, the counters, the report ----

/// What the fold remembers of a source.
#[derive(Debug, Clone, Default, PartialEq)]
struct Seen {
    etag: Option<String>,
    modified: Option<String>,
    latest: Option<String>,
    ids: Vec<String>,
    moved: Option<String>,
}

impl Seen {
    fn of(v: &Value) -> Option<Seen> {
        let Value::Map(m) = v else { return None };
        let text = |k: &str| match m.get(k) {
            Some(Value::Str(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let ids = match m.get("ids") {
            Some(Value::List(l)) => l.iter().filter_map(|i| if let Value::Str(s) = i { Some(s.clone()) } else { None }).collect(),
            _ => Vec::new(),
        };
        Some(Seen { etag: text("etag"), modified: text("modified"), latest: text("latest"), ids, moved: text("moved") })
    }

    fn json(&self, source: &str) -> Json {
        json!({ "source": source, "etag": self.etag, "modified": self.modified, "latest": self.latest, "ids": self.ids, "moved": self.moved })
    }
}

#[derive(Default, Clone, Copy)]
struct Counts {
    polls: u64,
    quiet: u64,
    changed: u64,
    failed: u64,
}

/// What a pass found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    /// Sources asked.
    pub asked: usize,
    /// Answered 304 or had nothing new.
    pub quiet: usize,
    /// One line for each change, move, gap, cooling or failure.
    pub lines: Vec<String>,
}

/// Where a tool stands: what runs here, the latest release seen, and what J4n has tested.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub of: String,
    /// `None`: no binary is followed for it.
    pub running: Option<Option<String>>,
    pub latest: Option<String>,
    pub tested: Option<String>,
}

impl Standing {
    pub fn line(&self) -> String {
        let name = self.of.strip_prefix("connector:").unwrap_or(&self.of);
        let dash = || DASH.to_string();
        let running = match &self.running {
            None => dash(),
            Some(None) => "not installed".into(),
            Some(Some(v)) => v.clone(),
        };
        let behind = match (&self.running, &self.latest) {
            (Some(Some(r)), Some(l)) if compare(r, l) == Ordering::Less => " (behind)",
            _ => "",
        };
        format!("{name}: running {running} · latest {} · tested {}{behind}", self.latest.clone().unwrap_or_else(dash), self.tested.clone().unwrap_or_else(dash))
    }
}

/// The dash that says nothing is known.
const DASH: &str = "—";

// ---- the watcher ----

/// What follows the sources.
pub struct Watcher {
    net: Net,
    log: Log,
    folds: Folds,
    /// The settings as they are now: `watch.upstream` and the rest are read per tick.
    settings: j4n_core::setting::Live,
    sources: Vec<Source>,
    rest: String,
    pace: Duration,
    budget: Mutex<Budget>,
    counts: Mutex<(Counts, u64)>,
    probed: Mutex<BTreeMap<String, Option<String>>>,
    next: Mutex<BTreeMap<Tier, u64>>,
    /// The names J4n has for each thing followed (`of`), and where a proposal goes.
    uses: BTreeMap<String, Uses>,
    proposer: OnceLock<Arc<Proposer>>,
    /// Whether the digest of `watch.digest = open` was said in this run.
    opened: AtomicBool,
}

impl Watcher {
    pub fn new(net: Net, log: &Log, folds: &Folds, settings: impl Into<j4n_core::setting::Live>, sources: Vec<Source>) -> Watcher {
        Watcher {
            net: net.no_redirects(),
            log: log.clone(),
            folds: folds.clone(),
            settings: settings.into(),
            sources,
            rest: "https://api.github.com".into(),
            pace: Duration::from_secs(1),
            budget: Mutex::default(),
            counts: Mutex::default(),
            probed: Mutex::default(),
            next: Mutex::default(),
            uses: BTreeMap::new(),
            proposer: OnceLock::new(),
            opened: AtomicBool::new(false),
        }
    }

    /// The names J4n holds for each thing followed, by its `of` (`connector:claude`): what a release is read against.
    pub fn with_uses(mut self, uses: BTreeMap<String, Uses>) -> Watcher {
        self.uses = uses;
        self
    }

    /// Where "mark it tested" is proposed, once the front exists.
    pub fn propose_to(&self, p: Arc<Proposer>) {
        let _ = self.proposer.set(p);
    }

    /// Where GitHub's REST is (a test's server), and the pause between two requests to one host.
    pub fn with(mut self, rest: &str, pace: Duration) -> Watcher {
        (self.rest, self.pace) = (rest.into(), pace);
        self
    }

    pub fn on(&self) -> bool {
        self.settings.flag("watch.upstream", false)
    }

    fn now(&self) -> u64 {
        self.log.clock().now_ms()
    }

    fn put(&self, kind: &str, data: Json) {
        let _ = self.log.append(New::of(kind, Actor::Core, data));
    }

    fn seen(&self, id: &str) -> Option<Seen> {
        Seen::of(&self.folds.read("watch_seen", &[Key::Name(id.into())], self.now()))
    }

    fn interval(&self, tier: Tier) -> u64 {
        let (id, or) = match tier {
            Tier::Hot => ("watch.hot_min", 30.0),
            Tier::Warm => ("watch.warm_min", 240.0),
            Tier::Cold => ("watch.cold_min", 1440.0),
        };
        (self.settings.num(id, or) * 60_000.0) as u64
    }

    /// The tiers whose time has come at `now`, each looked at, and the time to the next look. The first time, a tier is due `interval`
    /// after the last pass the Log counted, so a J4n that was closed for a week looks once at start and one closed for a minute does not.
    pub fn tick(&self, now: u64) -> u64 {
        if !self.on() {
            return 60_000;
        }
        let last = match self.folds.read("watch_pass", &[], now) {
            Value::Num(n) => Some(n as u64),
            _ => None,
        };
        let mut due = Vec::new();
        {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            for tier in [Tier::Hot, Tier::Warm, Tier::Cold] {
                let at = *next.entry(tier).or_insert_with(|| last.map_or(now, |l| l + self.interval(tier)));
                if at <= now {
                    let jitter = (now ^ (tier as u64 * 7919)) % 21; // ±10 % of the interval, so the tiers do not march together
                    next.insert(tier, now + self.interval(tier) * (90 + jitter) / 100);
                    due.push(tier);
                }
            }
        }
        for tier in due {
            self.pass(Some(tier), now);
        }
        self.digest(now);
        let next = self.next.lock().unwrap_or_else(|e| e.into_inner());
        next.values().min().map_or(60_000, |at| at.saturating_sub(now))
    }

    /// Start looking on a timer of its own while the process lives (nothing is asked while the switch is off; the timer idles, so a switch
    /// turned on while J4n runs is followed).
    pub fn start(this: &Arc<Watcher>) {
        let me: Weak<Watcher> = Arc::downgrade(this);
        let _ = std::thread::Builder::new().name("j4n-watch".into()).spawn(move || {
            while let Some(w) = me.upgrade() {
                let wait = w.tick(w.now()).clamp(1_000, 5_000);
                drop(w);
                std::thread::sleep(Duration::from_millis(wait));
            }
        });
    }

    /// Look at the feeds of `tier` (all of them: `None`), a few hosts at once. No program is run: a probe is run when the standing is
    /// asked for (`standing`), never by a timer, so a start that is asked for nothing runs none.
    pub fn pass(&self, tier: Option<Tier>, now: u64) -> Report {
        let mut groups: BTreeMap<String, Vec<&Source>> = BTreeMap::new();
        let issues = self.settings.flag("watch.issues", true);
        for s in self.sources.iter().filter(|s| s.adapter != Adapter::Probe && (issues || s.adapter != Adapter::Issue) && tier.is_none_or(|t| s.tier == t)) {
            groups.entry(self.target(s).as_deref().map(host).unwrap_or_default()).or_default().push(s);
        }
        let groups: Vec<Vec<&Source>> = groups.into_values().collect();
        let (index, report) = (AtomicUsize::new(0), Mutex::new(Report::default()));
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL.min(groups.len()) {
                scope.spawn(|| {
                    while let Some(group) = groups.get(index.fetch_add(1, Atomic::SeqCst)) {
                        for (i, s) in group.iter().enumerate() {
                            if i > 0 {
                                std::thread::sleep(self.pace);
                            }
                            let said = self.poll(s, now);
                            let mut r = report.lock().unwrap_or_else(|e| e.into_inner());
                            r.asked += 1;
                            match said {
                                Some(line) => r.lines.push(line),
                                None => r.quiet += 1,
                            }
                        }
                    }
                });
            }
        });
        let report = report.into_inner().unwrap_or_else(|e| e.into_inner());
        self.count(now);
        report
    }

    /// The counters, written at most once an hour and only when something was asked.
    fn count(&self, now: u64) {
        let mut c = self.counts.lock().unwrap_or_else(|e| e.into_inner());
        if c.0.polls > 0 && now.saturating_sub(c.1) >= HOUR_MS {
            self.put("watch.counts", json!({ "polls": c.0.polls, "quiet": c.0.quiet, "changed": c.0.changed, "failed": c.0.failed }));
            *c = (Counts::default(), now);
        }
    }

    fn tally(&self, f: impl FnOnce(&mut Counts)) {
        f(&mut self.counts.lock().unwrap_or_else(|e| e.into_inner()).0);
    }

    /// Ask one source; the line to say if something came of it.
    fn poll(&self, s: &Source, now: u64) -> Option<String> {
        let old = self.seen(&s.id);
        // a source that moved is asked where it went
        let target = self.target(s);
        let url = old.as_ref().and_then(|o| o.moved.as_deref()).or(target.as_deref())?;
        let host = host(url);
        if self.budget.lock().unwrap_or_else(|e| e.into_inner()).cools(&host, now) || (s.adapter == Adapter::Issue && !self.rest_slot(&host, now)) {
            return None;
        }
        self.tally(|c| c.polls += 1);
        let (etag, modified) = old.as_ref().map_or((None, None), |o| (o.etag.clone(), o.modified.clone()));
        let mut headers = vec![("Accept", more::accept(s.adapter))];
        if let Some(e) = etag.as_deref() {
            headers.push(("If-None-Match", e));
        } else if let Some(m) = modified.as_deref() {
            headers.push(("If-Modified-Since", m));
        }
        let mut reply = match self.net.get(url, &headers) {
            Ok(r) => r,
            Err(e) => {
                self.tally(|c| c.failed += 1);
                return Some(format!("{}: not reached ({e})", s.id));
            }
        };
        if s.adapter == Adapter::Issue {
            self.rest_left(&reply);
        }
        let (mut moved_line, mut moved_to) = (None, old.as_ref().and_then(|o| o.moved.clone()));
        if matches!(reply.status, 301 | 308) {
            let mut to = reply.headers.get("location").cloned().unwrap_or_default();
            if to.starts_with('/') {
                // a path on the same host: the answer to a request for `url`
                let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
                to = format!("{scheme}://{}{to}", rest.split('/').next().unwrap_or_default());
            }
            if !to.is_empty() {
                // followed once, said once, and proposed as the new `url`; the row is not rewritten
                if moved_to.as_deref() != Some(to.as_str()) {
                    self.put("watch.moved", json!({ "source": s.id, "from": url, "to": to }));
                    self.propose_moved(s, &to);
                    moved_line = Some(format!("{}: moved to {to}", s.id));
                    moved_to = Some(to.clone());
                }
                reply = match self.net.get(&to, &headers[..1]) {
                    Ok(r) => r,
                    Err(e) => return Some(format!("{}: moved to {to}, which was not reached ({e})", s.id)),
                };
            }
        }
        if matches!(reply.status, 403 | 429 | 503) || (reply.headers.contains_key("retry-after") && reply.status >= 400) {
            let wait = reply.headers.get("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map_or(HOUR_MS, |s| s * 1000);
            self.budget.lock().unwrap_or_else(|e| e.into_inner()).cooling.insert(host.clone(), now + wait);
            self.put("watch.cooling", json!({ "host": host, "until_ms": now + wait, "status": reply.status }));
            self.tally(|c| c.failed += 1);
            return Some(format!("{host}: asked to wait ({})", reply.status));
        }
        if reply.status == 304 {
            self.tally(|c| c.quiet += 1);
            return moved_line;
        }
        if reply.status != 200 {
            self.tally(|c| c.failed += 1);
            return Some(format!("{}: answered {}", s.id, reply.status));
        }
        let (new_etag, new_modified) = (reply.headers.get("etag").cloned(), reply.headers.get("last-modified").cloned());
        let Ok(text) = reply.text() else {
            self.tally(|c| c.failed += 1);
            return Some(format!("{}: the answer broke off", s.id));
        };
        let stamp = (new_etag, new_modified, moved_to);
        let line = match s.adapter {
            Adapter::Models | Adapter::Page => self.looked(s, old, &text, stamp),
            Adapter::Issue => self.issue(s, old, &text, stamp, now),
            _ => self.changed(s, old, entries(s.adapter, &text), stamp, now),
        };
        moved_line.into_iter().chain(line).reduce(|a, b| format!("{a}; {b}"))
    }

    /// An answer of 200: the releases that are new, once each, and what to remember.
    fn changed(&self, s: &Source, old: Option<Seen>, feed: Vec<Entry>, (etag, modified, moved): (Option<String>, Option<String>, Option<String>), now: u64) -> Option<String> {
        let prev = old.clone().unwrap_or_default();
        let known = |e: &Entry| prev.ids.contains(&e.id);
        let mut fresh: Vec<Entry> = feed.iter().filter(|e| !known(e)).cloned().collect();
        let mut said = Vec::new();
        // a feed holds ten entries: when none of them is one seen, releases may have slipped out between two looks
        if old.is_some() && !prev.ids.is_empty() && !feed.is_empty() && fresh.len() == feed.len() {
            self.put("watch.gap", json!({ "source": s.id, "last_seen": prev.latest, "oldest": feed.last().map(|e| e.id.clone()) }));
            said.push(format!("{}: a gap since {}", s.id, prev.latest.as_deref().unwrap_or(DASH)));
            if let Some(more) = self.page(s, now) {
                for e in more.into_iter().filter(|e| !known(e)) {
                    if !fresh.iter().any(|f| f.id == e.id) {
                        fresh.push(e);
                    }
                }
            }
        }
        let stable: Vec<&Entry> = fresh.iter().filter(|e| s.channel.admits(channel_of(&e.id))).collect();
        let latest = feed.iter().chain(fresh.iter()).filter(|e| s.channel.admits(channel_of(&e.id))).map(|e| e.version.clone()).max_by(|a, b| compare(a, b)).or(prev.latest.clone());
        // the first look only says where things stand; nothing in it is new
        if old.is_some() {
            for e in stable.iter().rev() {
                let a = relevance::assess(&e.notes, self.uses.get(&s.of).unwrap_or(&Uses::default()));
                let items: Vec<Json> = a.items.iter().map(|(c, text, names)| json!({ "class": c.word(), "text": text, "names": names })).collect();
                let class = a.class.unwrap_or(Class::Noise);
                let lead = a.items.first().map(|i| digest::one_line(&i.1, 160)).unwrap_or_default();
                self.put("watch.release", json!({ "source": s.id, "of": s.of, "version": e.version, "tag": e.id, "link": e.link, "published": e.published, "class": class.word(), "counts": a.counts, "items": items, "security": a.security, "lead": lead, "at_ms": now }));
                said.push(format!("{}: {} is out{}", s.id, e.version, if class >= Class::Touches { format!(" ({})", class.word()) } else { String::new() }));
                if class >= Class::Touches && self.settings.flag("watch.canary", false) {
                    said.extend(self.canary(s, &e.version));
                }
            }
        }
        let mut ids: Vec<String> = Vec::new();
        for id in feed.iter().map(|e| &e.id).chain(prev.ids.iter()) {
            if !ids.contains(id) && ids.len() < KEEP_IDS {
                ids.push(id.clone());
            }
        }
        let first = self.keep(&s.id, &old, Seen { etag, modified, latest, ids, moved });
        if first {
            return None;
        }
        (!said.is_empty()).then(|| said.join("; "))
    }

    /// One page of GitHub's REST releases for a feed that slipped, if the floor lets one go.
    fn page(&self, s: &Source, now: u64) -> Option<Vec<Entry>> {
        // `https://github.com/<owner>/<repo>/releases.atom`
        let mut parts = s.url.as_deref()?.split("/releases.atom").next()?.rsplit('/');
        let (repo, owner) = (parts.next()?, parts.next()?);
        let rest_host = host(&self.rest);
        if !self.budget.lock().unwrap_or_else(|e| e.into_inner()).rest_ok(&rest_host, now) {
            return None;
        }
        self.budget.lock().unwrap_or_else(|e| e.into_inner()).rest.push_back(now);
        let reply = self.net.get(&format!("{}/repos/{owner}/{repo}/releases?per_page=30", self.rest), &[("Accept", "application/vnd.github+json")]).ok()?;
        let mut b = self.budget.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(r) = reply.headers.get("x-ratelimit-remaining").and_then(|v| v.parse().ok()) {
            b.remaining = Some(r);
        }
        if matches!(reply.status, 403 | 429) || (reply.headers.contains_key("retry-after") && reply.status >= 400) {
            let wait = reply.headers.get("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map_or(HOUR_MS, |s| s * 1000);
            b.cooling.insert(rest_host.clone(), now + wait);
            drop(b);
            self.put("watch.cooling", json!({ "host": rest_host, "until_ms": now + wait, "status": reply.status }));
            return None;
        }
        drop(b);
        reply.ok().then(|| reply.text().ok()).flatten().map(|t| rest_releases(&t))
    }

    // ---- the canary and the sets ----

    /// Run the scripted run of release `version` in a folder of its own and compare its shape with the one known to be good. Equal (or only
    /// grown) proposes "tested"; different is written for the instant item; nothing is installed over the operator's tool.
    fn canary(&self, s: &Source, version: &str) -> Option<String> {
        let argv = s.canary.as_ref()?;
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("j4n-canary-{}-{}-{}", std::process::id(), N.fetch_add(1, Atomic::SeqCst), s.id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        let ran = s.install.as_ref().map_or(Ok(String::new()), |i| canary::run(&canary::fill(i, version, &dir), &dir, CANARY_WAIT)).and_then(|_| canary::run(&canary::fill(argv, version, &dir), &dir, CANARY_WAIT));
        let _ = std::fs::remove_dir_all(&dir);
        let say = |outcome: &str, more: Json, lead: &str| self.put("watch.canary", json!({ "source": s.id, "of": s.of, "version": version, "outcome": outcome, "more": more, "lead": digest::one_line(lead, 160) }));
        let now = match ran {
            Ok(out) => canary::shape_of(&out),
            Err(why) => {
                say("failed", json!(why), &why);
                return Some(format!("{}: the canary on {version} failed ({why})", s.id));
            }
        };
        let good = match s.record.as_deref() {
            Some(file) => std::fs::read_to_string(file).ok().map(|t| canary::shape_of(&t)),
            None => match self.folds.read("watch_shape", &[Key::Name(s.of.clone())], self.now()) {
                Value::Map(m) => match m.get("shape") {
                    Some(Value::List(l)) => Some(l.iter().filter_map(|v| if let Value::Str(p) = v { Some(p.clone()) } else { None }).collect()),
                    _ => None,
                },
                _ => None,
            },
        };
        let clip = |v: &[String]| v.iter().take(20).cloned().collect::<Vec<_>>();
        let verdict = good.as_ref().map(|g| canary::compare(g, &now));
        match verdict {
            None => {
                self.put("watch.shape", json!({ "of": s.of, "version": version, "shape": now }));
                say("baseline", Json::Null, "");
                Some(format!("{}: the canary on {version} has no good run to compare with; its shape is kept", s.id))
            }
            Some(Verdict::Differs { gone, new }) => {
                say("differs", json!({ "gone": clip(&gone), "new": clip(&new) }), gone.first().or(new.first()).map_or("", String::as_str));
                Some(format!("{}: {version} says it differently ({} gone, {} new)", s.id, gone.len(), new.len()))
            }
            Some(v) => {
                let grew = if let Verdict::Grew(n) = &v { clip(n) } else { Vec::new() };
                self.put("watch.shape", json!({ "of": s.of, "version": version, "shape": now }));
                say(if grew.is_empty() { "same" } else { "grew" }, json!(grew), "");
                self.propose_tested(s, version);
                Some(format!("{}: {version} says the same as the good run", s.id))
            }
        }
    }

    /// "Mark it tested": a step that sets `tested` on the row of the binary installed here (else the first row of the thing).
    fn propose_tested(&self, s: &Source, version: &str) {
        let Some(p) = self.proposer.get() else { return };
        let row = self.sources.iter().find(|r| r.of == s.of && r.adapter == Adapter::Probe).unwrap_or(s);
        let name = s.of.strip_prefix("connector:").unwrap_or(&s.of);
        let mut fields = toml::Table::new();
        fields.insert("tested".into(), toml::Value::String(version.into()));
        let draft = Draft {
            key: format!("tested:{}:{version}", s.of),
            title: format!("Mark {name} {version} as tested"),
            why: format!("The scripted run of {name} {version}, made in a folder of its own, says what the good run says (the events and the names and types of their fields). Nothing was installed over the one here."),
            evidence: 1,
            steps: vec![Step { to: j4n_core::change::Where::User, file: Some("sources/tested.toml".into()), kind: "source".into(), id: row.id.clone(), op: StepOp::Set(fields) }],
            widens: Vec::new(),
        };
        let _ = p.make(draft, Actor::Core, None);
    }

    /// A set of names that a thing offers (the tools of a server, the models of a provider, each with a hash of what it is) as it is now:
    /// what changed since the last look is written, a name gone or changed as `breaks`. The first look only says where things stand.
    pub fn observe(&self, of: &str, set: &BTreeMap<String, String>) -> Vec<Change> {
        self.observe_in(of, set, Class::Breaks)
    }

    /// As [`Watcher::observe`], but no change counts for more than `most`: a docs page that is gone is news, not a break.
    fn observe_in(&self, of: &str, set: &BTreeMap<String, String>, most: Class) -> Vec<Change> {
        let old: Option<BTreeMap<String, String>> = match self.folds.read("watch_set", &[Key::Name(of.into())], self.now()) {
            Value::Map(m) => match m.get("set") {
                Some(Value::Map(s)) => Some(s.iter().map(|(k, v)| (k.clone(), if let Value::Str(h) = v { h.clone() } else { String::new() })).collect()),
                _ => None,
            },
            _ => None,
        };
        let changes = old.as_ref().map(|o| relevance::diff_set(o, set)).unwrap_or_default();
        if old.is_none() || !changes.is_empty() {
            self.put("watch.set", json!({ "of": of, "set": set }));
        }
        if !changes.is_empty() {
            let named = |w: &str| changes.iter().filter(|c| c.word() == w).map(|c| c.name().to_string()).collect::<Vec<_>>();
            let class = changes.iter().map(Change::class).max().unwrap_or(Class::Noise).min(most);
            let lead = ["gone", "changed", "added"].iter().find_map(|w| named(w).into_iter().next()).unwrap_or_default();
            self.put("watch.diff", json!({ "of": of, "class": class.word(), "gone": named("gone"), "changed": named("changed"), "added": named("added"), "lead": digest::one_line(&lead, 160) }));
        }
        changes
    }

    // ---- the digest and "plan it" ----

    /// The releases that touch or break something J4n uses, each once, the newest first.
    pub fn items(&self) -> Vec<Item> {
        let Value::Map(rows) = self.folds.read("watch_items", &[], self.now()) else { return Vec::new() };
        let mut out: Vec<Item> = rows.iter().filter_map(|(id, row)| Item::of_row(id, row)).collect();
        out.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// One line for what came since the last one, when `watch.digest` says it is time (it is a notice by a rule; the Upstream tab lists
    /// the items whenever it is looked at).
    pub fn digest(&self, now: u64) -> bool {
        let cadence = Cadence::word(&self.settings.word("watch.digest", "weekly"));
        let last = match self.folds.read("watch_digest", &[], now) {
            Value::Num(n) => n as u64,
            _ => 0,
        };
        let waiting: Vec<u64> = self.items().iter().filter(|i| i.at_ms > last).map(|i| i.at_ms).collect();
        if cadence.due(&waiting, self.opened.load(Atomic::SeqCst), now) {
            self.opened.store(true, Atomic::SeqCst);
            self.put("watch.digest", json!({ "items": waiting.len(), "since_ms": last, "at_ms": now }));
            return true;
        }
        false
    }

    /// "Plan it": the proposal that puts a note about item `id` on the operator's list when they accept it.
    pub fn plan_it(&self, id: &str) -> Result<String, String> {
        let item = self.items().into_iter().find(|i| i.id == id).ok_or_else(|| format!("no release is listed as `{id}`"))?;
        let p = self.proposer.get().ok_or("nothing can be proposed here")?;
        match p.make(item.plan(), Actor::Core, None) {
            Ok(Some(pid)) => Ok(format!("proposed as {pid}: `:accept {pid}` puts a note about {} {} on your list", item.source, item.version)),
            Ok(None) => Err("that is already proposed, or was turned down".into()),
            Err(e) => Err(e.msg),
        }
    }

    /// A source that answered "moved": proposed as its new `url`, widening what J4n asks (the host is new), and never rewritten by itself.
    fn propose_moved(&self, s: &Source, to: &str) {
        let (Some(p), true) = (self.proposer.get(), to.starts_with("https://") || to.starts_with("http://")) else { return };
        let mut fields = toml::Table::new();
        fields.insert("url".into(), toml::Value::String(to.into()));
        let draft = Draft {
            key: format!("moved:{}", s.id),
            title: format!("Follow {} at {}", s.id, digest::one_line(to, 100)),
            why: format!("{} answered that it lives at another address. J4n follows it there for now and says so; this makes it the row's own address.", s.id),
            evidence: 1,
            steps: vec![Step { to: j4n_core::change::Where::User, file: Some("sources/moved.toml".into()), kind: "source".into(), id: s.id.clone(), op: StepOp::Set(fields) }],
            widens: vec![format!("asks {} for releases{}", host(to), if to.starts_with("http://") { ", not encrypted" } else { "" })],
        };
        let _ = p.make(draft, Actor::Core, None);
    }

    // ---- what is installed here ----

    /// The version a binary says (`program --version`), `None` when it is not there or says none.
    fn version_of(program: &str) -> Option<String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let launch = j4n_core::place::Launch::new(vec![program.to_string(), "--version".into()], cwd, BTreeMap::new());
        let (mut text, exit) = crate::proc::capture(&launch, PROBE_WAIT).ok()?;
        if exit.killed {
            return None;
        }
        text.push('\n');
        text.push_str(&exit.tail);
        text.split_whitespace().find_map(|w| split_version(w).map(|(d, rest)| format!("{d}{rest}")))
    }

    fn probe_all(&self) {
        for s in self.sources.iter().filter(|s| s.adapter == Adapter::Probe) {
            let v = s.program.as_deref().and_then(Self::version_of);
            self.probed.lock().unwrap_or_else(|e| e.into_inner()).insert(s.of.clone(), v);
        }
    }

    /// Where each followed tool stands, from the local probe and the Log; no request leaves.
    pub fn standing(&self) -> Vec<Standing> {
        self.probe_all();
        let mut of: Vec<&str> = self.sources.iter().map(|s| s.of.as_str()).collect();
        of.sort_unstable();
        of.dedup();
        let probed = self.probed.lock().unwrap_or_else(|e| e.into_inner()).clone();
        of.into_iter()
            .map(|o| Standing {
                of: o.into(),
                running: probed.get(o).cloned(),
                latest: self.sources.iter().filter(|s| s.of == o && s.adapter != Adapter::Probe).filter_map(|s| self.seen(&s.id)?.latest).max_by(|a, b| compare(a, b)),
                tested: self.sources.iter().filter(|s| s.of == o).filter_map(|s| s.tested.clone()).max_by(|a, b| compare(a, b)),
            })
            .collect()
    }
}

impl Upstream for Watcher {
    fn show(&self) -> String {
        let lines: Vec<String> = self.standing().iter().map(Standing::line).collect();
        if lines.is_empty() { "no tool is followed: a `source` row names one".into() } else { lines.join("\n") }
    }

    fn once(&self) -> Result<String, String> {
        if !self.on() {
            return Err("`watch.upstream` is off, so nothing is asked: turn it on in Settings › All › watch.upstream, and J4n follows the tools by itself".into());
        }
        let r = self.pass(None, self.now());
        let changes = if r.lines.is_empty() { "nothing changed".to_string() } else { r.lines.join("\n") };
        Ok(format!("asked {} source(s), {} quiet\n{changes}\n{}", r.asked, r.quiet, self.show()))
    }

    fn plan(&self, id: &str) -> Result<String, String> {
        self.plan_it(id)
    }
}
