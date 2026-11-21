//! The tools J4n follows are asked quietly. The server is the loopback mock, the Log, the
//! folds and the Table are the real ones, the clock is moved by hand.

use crate::mock::{Mock, Say, Seen};
use crate::world::Scratch;
use j4n_connect::net::Net;
use j4n_connect::watch::{Budget, Channel, Source, SourceRow, Watcher, atom, channel_of, compare};
use j4n_core::clock::ManualClock;
use j4n_core::fold::{Folds, declared};
use j4n_core::front::Upstream;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Kind, Layer, LayerKind, SourceFile, Tables};
use serde_json::{Value as Json, json};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct Bed {
    _dir: Scratch,
    pub(crate) log: Log,
    clock: Arc<ManualClock>,
    pub(crate) folds: Folds,
    pub(crate) tables: Tables,
    settings: Settings,
    pub(crate) proposals: j4n_core::proposal::Proposals,
    pub(crate) changes: j4n_core::changes::Changes,
}

pub(crate) fn file(path: &str, text: &str) -> SourceFile {
    SourceFile { path: path.into(), text: text.into() }
}

/// A Log, the folds of the base pack's `watch.toml`, and the Table with the operator's `rows` and settings on top.
pub(crate) fn bed(rows: &str, on: bool) -> Bed {
    bed_with(rows, on, "")
}

/// As [`bed`], with more of the operator's settings (`[[setting]]` tables).
pub(crate) fn bed_with(rows: &str, on: bool, more: &str) -> Bed {
    let dir = Scratch::new("watch");
    let clock = Arc::new(ManualClock::at(1_000_000_000));
    let (log, problems) = Log::open(&dir.join(".j4n"), Scope::Project, Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() }, Redactor::new()).unwrap();
    assert!(problems.is_empty());
    let pack = Layer::new(LayerKind::Pack, "w", vec![file("folds/watch.toml", include_str!("../../../../packs/base/folds/watch.toml")), file("settings/watch.toml", include_str!("../../../../packs/base/settings/watch.toml"))]);
    let switch = format!("api = 1\n[[setting]]\nid = \"watch.upstream\"\nvalue = {on}\n{more}");
    let user = Layer::new(LayerKind::User, "home", vec![file("sources/mine.toml", &format!("api = 1\n{rows}")), file("settings/on.toml", &switch)]);
    let (tables, problems) = Tables::load(vec![pack, user]);
    assert!(problems.is_empty(), "{problems:?}");
    let (settings, problems) = Settings::load(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let folds = Folds::new(&dir.join("folds"));
    let (declared, problems) = declared(&tables, Scope::Project);
    assert!(problems.is_empty(), "{problems:?}");
    for f in declared {
        assert!(folds.add(f, 0).is_empty());
    }
    let (proposals, changes) = (j4n_core::proposal::Proposals::new(), j4n_core::changes::Changes::new());
    assert!(folds.add(Box::new(changes.clone()), 0).is_empty() && folds.add(Box::new(proposals.clone()), 0).is_empty());
    folds.attach(&log).unwrap();
    Bed { _dir: dir, log, clock, folds, tables, settings, proposals, changes }
}

impl Bed {
    pub(crate) fn watcher(&self, rest: &str) -> Watcher {
        let (sources, problems) = Source::load(&self.tables);
        assert!(problems.is_empty(), "{problems:?}");
        Watcher::new(Net::new(), &self.log, &self.folds, &self.settings, sources).with(rest, Duration::ZERO)
    }

    /// As [`Bed::watcher`], with this pause between two requests to one host.
    pub(crate) fn watcher_paced(&self, rest: &str, pace: Duration) -> Watcher {
        let (sources, problems) = Source::load(&self.tables);
        assert!(problems.is_empty(), "{problems:?}");
        Watcher::new(Net::new(), &self.log, &self.folds, &self.settings, sources).with(rest, pace)
    }

    pub(crate) fn events(&self, kind: &str) -> Vec<Json> {
        self.log.sync();
        self.log.read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
    }

    pub(crate) fn at(&self, ms: u64) {
        self.clock.set(ms);
    }
}

pub(crate) fn row(id: &str, url: &str, extra: &str) -> String {
    format!("[[source]]\nid = \"{id}\"\nof = \"tool\"\nadapter = \"atom\"\nurl = \"{url}\"\n{extra}\n")
}

/// An Atom feed of GitHub's shape: the tags, newest first.
fn feed(tags: &[&str]) -> String {
    let entries: String = tags.iter().map(|t| format!("<entry><id>tag:github.com,2008:Repository/1/{t}</id><updated>2026-10-29T10:00:00Z</updated><link rel=\"alternate\" type=\"text/html\" href=\"https://github.com/o/r/releases/tag/{t}\"/><title>{t}</title></entry>")).collect();
    format!("<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Releases</title>{entries}</feed>")
}

/// A server that answers a feed with an ETag the way GitHub does: 200 once, then 304 for a request that carries it.
fn feeds(tags: Arc<std::sync::Mutex<Vec<&'static str>>>) -> Mock {
    Mock::start(move |seen: &Seen, _| {
        let tags = tags.lock().unwrap().clone();
        let etag = format!("\"{}\"", tags.join(","));
        if seen.headers.get("if-none-match") == Some(&etag) {
            return Say::Headed(304, vec![], String::new());
        }
        Say::Headed(200, vec![("ETag", etag), ("Content-Type", "application/atom+xml".into())], feed(&tags))
    })
}

fn count(m: &Mock, path: &str) -> usize {
    m.seen().iter().filter(|s| s.path.starts_with(path)).count()
}

#[test]
fn a_second_look_at_a_source_that_did_not_change_is_a_304_and_writes_nothing() {
    let tags = Arc::new(std::sync::Mutex::new(vec!["v2.1.311", "v2.1.310"]));
    let m = feeds(tags);
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    let first = w.pass(None, 1);
    assert_eq!((first.asked, first.lines.len()), (1, 0), "the first look only says where things stand: {first:?}");
    let written = b.events("watch.seen").len();
    assert_eq!(written, 1);
    let second = w.pass(None, 2);
    assert_eq!((second.asked, second.quiet, second.lines.len()), (1, 1, 0), "{second:?}");
    assert_eq!(m.seen()[1].headers.get("if-none-match").map(String::as_str), Some("\"v2.1.311,v2.1.310\""), "the validator went with it");
    let all: Vec<String> = { b.log.sync(); b.log.read(0).filter(|e| e.kind.starts_with("watch.")).map(|e| e.kind).collect() };
    assert_eq!(all, ["watch.seen"], "a 304 writes no event: {all:?}");
}

#[test]
fn a_changed_feed_writes_one_release_event_for_each_new_release_once() {
    let tags = Arc::new(std::sync::Mutex::new(vec!["v2.1.311", "v2.1.310"]));
    let m = feeds(tags.clone());
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    *tags.lock().unwrap() = vec!["v2.1.313", "v2.1.312", "v2.1.311", "v2.1.310"];
    let r = w.pass(None, 2);
    assert_eq!(r.lines.len(), 1, "{r:?}");
    let got = b.events("watch.release");
    assert_eq!(got.iter().map(|g| g["version"].as_str().unwrap()).collect::<Vec<_>>(), ["2.1.312", "2.1.313"], "oldest first: {got:?}");
    assert_eq!((got[0]["source"].as_str(), got[0]["of"].as_str(), got[0]["link"].as_str()), (Some("t"), Some("tool"), Some("https://github.com/o/r/releases/tag/v2.1.312")));
    // the validator is written once, with the change; the same feed again is a 304 and says nothing more
    assert_eq!((b.events("watch.seen").len(), b.events("watch.gap").len()), (2, 0), "two new releases among ones seen are not a gap");
    w.pass(None, 3);
    assert_eq!((b.events("watch.release").len(), b.events("watch.seen").len()), (2, 2));
    // what the Log remembers is the Log's: a watcher of the same Log asks with the validator and finds nothing new
    let again = b.watcher(&m.url());
    assert_eq!(again.pass(None, 4).quiet, 1);
    assert_eq!(b.events("watch.release").len(), 2);
}

#[test]
fn a_feed_none_of_whose_entries_was_seen_is_a_gap_and_one_page_of_the_rest_fills_it() {
    let m = Mock::start(|seen: &Seen, n| {
        if seen.path.starts_with("/repos/o/r/releases") {
            let all: Vec<Json> = (3..=13).rev().map(|i| json!({ "tag_name": format!("v1.{i}.0"), "html_url": format!("https://x/v1.{i}.0"), "published_at": "2026-10-29T00:00:00Z", "prerelease": false })).collect();
            return Say::Headed(200, vec![("x-ratelimit-remaining", "55".into())], Json::Array(all).to_string());
        }
        // the first look: 1.1.0 and 1.2.0; the second: ten entries from 1.4.0 up, 1.3.0 slipped out of the feed
        let tags: Vec<String> = if n == 0 { vec!["v1.2.0".into(), "v1.1.0".into()] } else { (4..=13).rev().map(|i| format!("v1.{i}.0")).collect() };
        Say::Text(200, feed(&tags.iter().map(String::as_str).collect::<Vec<_>>()))
    });
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    let r = w.pass(None, 2);
    assert!(r.lines[0].contains("gap since 1.2.0"), "{r:?}");
    assert_eq!(b.events("watch.gap").len(), 1);
    assert_eq!(count(&m, "/repos/o/r/releases"), 1, "one page of the REST, no more");
    let got: Vec<String> = b.events("watch.release").iter().map(|e| e["version"].as_str().unwrap().to_string()).collect();
    assert!(got.contains(&"1.3.0".to_string()) && got.contains(&"1.13.0".to_string()) && !got.contains(&"1.2.0".to_string()), "{got:?}");
}

#[test]
fn a_prerelease_a_nightly_and_a_moving_tag_are_not_versions_on_the_stable_channel() {
    for (tag, want) in [("v2.1.312", Channel::Stable), ("rust-v0.164.0", Channel::Stable), ("rust-v0.165.0-alpha.1", Channel::Pre), ("v0.9.0-rc.2", Channel::Pre), ("v0.1.0-nightly.20261029", Channel::Nightly), ("nightly", Channel::Nightly), ("latest", Channel::Nightly), ("v2", Channel::Nightly), ("canary-2026", Channel::Nightly)] {
        assert_eq!(channel_of(tag), want, "{tag}");
    }
    assert_eq!([compare("2.1.9", "2.1.10"), compare("v3.0.0", "2.9.9"), compare("1.2.0", "1.2.0")], [Ordering::Less, Ordering::Greater, Ordering::Equal]);
    let m = Mock::start(|_, n| Say::Text(200, feed(&if n == 0 { vec!["v1.0.0"] } else { vec!["v1.1.0-alpha.1", "nightly", "v1.0.1", "v1.1.0-beta", "v1.0.0"] })));
    let b = bed(&format!("{}{}", row("stable", &format!("{}/o/r/releases.atom", m.url()), ""), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    w.pass(None, 2);
    assert_eq!(b.events("watch.release").iter().map(|e| e["version"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["1.0.1"], "the stable channel takes the one release");
    // the latest it knows is the stable one
    assert_eq!(w.standing()[0].latest.as_deref(), Some("1.0.1"));
    let atoms = atom(&feed(&["v1.0.0"]));
    assert_eq!((atoms[0].id.as_str(), atoms[0].version.as_str()), ("v1.0.0", "1.0.0"));
}

#[test]
fn a_301_is_followed_once_said_once_and_the_row_is_not_rewritten() {
    let m = Mock::start(|seen: &Seen, _| match seen.path.as_str() {
        "/old/r/releases.atom" => Say::Headed(301, vec![("Location", "/new/r/releases.atom".into())], String::new()),
        _ => Say::Text(200, feed(&["v1.0.0"])),
    });
    // the mock's redirect is relative to its own address
    let b = bed(&row("t", &format!("{}/old/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    let first = w.pass(None, 1);
    let moved = b.events("watch.moved");
    assert_eq!(moved.len(), 1, "{first:?} {moved:?}");
    assert!(moved[0]["from"].as_str().unwrap().ends_with("/old/r/releases.atom") && moved[0]["to"].as_str().unwrap().ends_with("/new/r/releases.atom"), "{moved:?}");
    assert!(b.events("watch.seen")[0]["moved"].as_str().unwrap().ends_with("/new/r/releases.atom"));
    // the next look goes where it went, and says nothing of it again
    let hits = count(&m, "/old/");
    w.pass(None, 2);
    assert_eq!((count(&m, "/old/"), b.events("watch.moved").len()), (hits, 1));
    // the row of the Table is what it was
    assert!(Source::load(&b.tables).0[0].url.as_deref().unwrap().ends_with("/old/r/releases.atom"));
}

#[test]
fn the_rest_never_passes_its_floor_and_a_host_that_says_wait_is_left_alone() {
    let mut budget = Budget::default();
    for t in 0..10 {
        assert!(budget.rest_ok("api.github.com", t), "{t}");
        budget.used(t);
    }
    assert!(!budget.rest_ok("api.github.com", 11), "ten an hour, no more");
    assert!(budget.rest_ok("api.github.com", 3_600_000 + 5), "an hour later the first ones are forgotten");
    // a sixth of what GitHub says is left
    let mut low = Budget::default();
    low.left(12);
    assert!(low.rest_ok("h", 0));
    low.used(0);
    low.used(1);
    assert!(!low.rest_ok("h", 2), "12 left is two requests");
    low.left(0);
    assert!(!Budget::default().cools("h", 0));
    let mut cool = Budget::default();
    cool.cool("h", 100);
    assert!((cool.cools("h", 99), cool.cools("h", 100), cool.rest_ok("h", 50)) == (true, false, false));
    // a 429 with Retry-After: the host is not asked again until then, and the Log says so
    let m = Mock::start(|_, _| Say::Headed(429, vec![("Retry-After", "120".into())], "slow down".into()));
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    let r = w.pass(None, 1_000_000);
    assert!(r.lines[0].contains("asked to wait"), "{r:?}");
    let cooling = b.events("watch.cooling");
    assert_eq!((cooling.len(), cooling[0]["until_ms"].as_u64(), cooling[0]["status"].as_u64()), (1, Some(1_120_000), Some(429)));
    let asked = m.seen().len();
    w.pass(None, 1_060_000);
    assert_eq!(m.seen().len(), asked, "within the wait nothing leaves");
    w.pass(None, 1_121_000);
    assert_eq!(m.seen().len(), asked + 1, "after it, one request");
}

#[test]
fn the_local_probe_says_running_latest_and_tested_and_a_missing_binary_is_not_installed() {
    let bin = Scratch::new("bin");
    let tool = bin.join("tool");
    std::fs::write(&tool, "#!/bin/sh\necho \"tool 2.1.310 (Claude Code)\"\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let tags = Arc::new(std::sync::Mutex::new(vec!["v2.1.312", "v2.1.311", "v2.1.310"]));
    let m = feeds(tags);
    let rows = format!("{}[[source]]\nid = \"bin\"\nof = \"tool\"\nadapter = \"probe\"\nprogram = \"{}\"\n[[source]]\nid = \"gone\"\nof = \"other\"\nadapter = \"probe\"\nprogram = \"{}\"\n", row("t", &format!("{}/o/r/releases.atom", m.url()), ""), tool.display(), bin.join("nothing").display());
    let b = bed(&rows, true);
    let w = b.watcher(&m.url());
    // before any look the latest is not known; nothing has left the machine for the standing
    let shown = w.show();
    assert!(shown.contains("tool: running 2.1.310 · latest — · tested —"), "{shown}");
    assert!(shown.contains("other: running not installed"), "a missing binary is not an error: {shown}");
    assert_eq!(m.seen().len(), 0);
    w.pass(None, 1);
    let shown = w.show();
    assert!(shown.contains("tool: running 2.1.310 · latest 2.1.312 · tested — (behind)"), "{shown}");
}

#[test]
fn with_the_switch_off_no_request_leaves_and_a_start_after_a_week_closed_looks_once() {
    let m = Mock::start(|_, _| Say::Text(200, feed(&["v1.0.0"])));
    let off = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), false);
    let w = off.watcher(&m.url());
    assert!(!w.on());
    w.tick(2_000_000_000);
    assert_eq!(m.seen().len(), 0, "off: nothing is asked");
    assert!(w.once().unwrap_err().contains("watch.upstream"), "and it says why");
    // on, and the Log has not counted a pass for a week: the first tick looks, and the next one a minute on does not
    let on = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let week = 7 * 86_400_000;
    on.log.append(New::of("watch.counts", j4n_core::change::Actor::Core, json!({ "polls": 3 }))).unwrap();
    on.log.sync();
    on.at(1_000_000_000 + week);
    let w = on.watcher(&m.url());
    w.tick(1_000_000_000 + week);
    assert_eq!(m.seen().len(), 1, "one catch-up look");
    w.tick(1_000_000_000 + week + 60_000);
    assert_eq!(m.seen().len(), 1, "and not another a minute later");
    // closed for a minute only: the interval has not passed, nothing is asked at start
    let recent = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    recent.log.append(New::of("watch.counts", j4n_core::change::Actor::Core, json!({ "polls": 3 }))).unwrap();
    recent.log.sync();
    recent.at(1_000_000_000 + 60_000);
    let before = m.seen().len();
    recent.watcher(&m.url()).tick(1_000_000_000 + 60_000);
    assert_eq!(m.seen().len(), before);
}

#[test]
fn the_other_adapters_read_npm_pypi_and_the_crates_index_and_a_source_of_an_absent_connector_is_left_out() {
    let newer = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = newer.clone();
    let m = Mock::start(move |seen: &Seen, _| {
        let n = if flag.load(std::sync::atomic::Ordering::SeqCst) { 9 } else { 0 };
        match seen.path.as_str() {
        "/npm/latest" => Say::Json(200, json!({ "name": "x", "version": if n < 3 { "1.0.0" } else { "1.1.0" } })),
        "/pypi.xml" => Say::Text(200, format!("<rss><channel><item><title>{}</title><link>https://pypi.org/project/x/</link><pubDate>Thu</pubDate></item><item><title>0.9</title></item></channel></rss>", if n < 3 { "0.9.1" } else { "0.9.2" })),
        _ => Say::Text(200, format!("{{\"vers\":\"0.1.0\",\"yanked\":false}}\n{{\"vers\":\"0.2.0\",\"yanked\":true}}\n{}", if n < 3 { String::new() } else { "{\"vers\":\"0.1.1\",\"yanked\":false}\n".to_string() })),
        }
    });
    let src = |id: &str, adapter: &str, path: &str| format!("[[source]]\nid = \"{id}\"\nof = \"{id}\"\nadapter = \"{adapter}\"\nurl = \"{}{path}\"\n", m.url());
    let rows = format!("{}{}{}[[source]]\nid = \"ghost\"\nof = \"connector:nothing\"\nadapter = \"probe\"\nprogram = \"ghost\"\n", src("n", "npm", "/npm/latest"), src("p", "pypi", "/pypi.xml"), src("c", "crates", "/crates/ab/cd/x"));
    let b = bed(&rows, true);
    let (sources, problems) = Source::load(&b.tables);
    assert!(problems.is_empty() && sources.len() == 3, "the ghost connector is not here to follow: {sources:?}");
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    w.pass(None, 2);
    let got: Vec<(String, String)> = b.events("watch.release").iter().map(|e| (e["source"].as_str().unwrap().into(), e["version"].as_str().unwrap().into())).collect();
    assert!(got.is_empty(), "nothing is new while nothing changed (the yanked one is not a version): {got:?}");
    let latest: Vec<Option<String>> = ["n", "p", "c"].iter().map(|id| w.standing().into_iter().find(|s| s.of == *id).and_then(|s| s.latest)).collect();
    assert_eq!(latest, [Some("1.0.0".into()), Some("0.9.1".into()), Some("0.1.0".into())]);
    newer.store(true, std::sync::atomic::Ordering::SeqCst);
    w.pass(None, 3);
    w.pass(None, 4);
    let mut got: Vec<(String, String)> = b.events("watch.release").iter().map(|e| (e["source"].as_str().unwrap().into(), e["version"].as_str().unwrap().into())).collect();
    got.sort();
    assert_eq!(got, [("c".into(), "0.1.1".into()), ("n".into(), "1.1.0".into()), ("p".into(), "0.9.2".into())]);
}

#[test]
fn a_row_that_cannot_be_used_is_a_problem_and_the_tiers_are_looked_at_by_their_own_intervals() {
    let bad = "[[source]]\nid = \"nourl\"\nof = \"x\"\nadapter = \"atom\"\n[[source]]\nid = \"noprog\"\nof = \"x\"\nadapter = \"probe\"\n";
    let dir = Scratch::new("watchbad");
    let user = Layer::new(LayerKind::User, "home", vec![file("sources/mine.toml", &format!("api = 1\n{bad}"))]);
    let (tables, _) = Tables::load(vec![user]);
    let (sources, problems) = Source::load(&tables);
    assert!(sources.is_empty() && problems.len() == 2, "{problems:?}");
    drop(dir);
    let m = Mock::start(|_, _| Say::Text(200, feed(&["v1.0.0"])));
    let rows = format!("{}{}", row("hot", &format!("{}/hot/r/releases.atom", m.url()), "tier = \"hot\""), row("cold", &format!("{}/cold/r/releases.atom", m.url()), "tier = \"cold\""));
    let b = bed(&rows, true);
    let w = b.watcher(&m.url());
    w.tick(1_000_000_000);
    assert_eq!((count(&m, "/hot/"), count(&m, "/cold/")), (1, 1), "the first time every tier is looked at");
    w.tick(1_000_000_000 + 10 * 60_000);
    assert_eq!(m.seen().len(), 2, "ten minutes on, nothing is due");
    w.tick(1_000_000_000 + 40 * 60_000);
    assert_eq!((count(&m, "/hot/"), count(&m, "/cold/")), (2, 1), "the hot tier at half an hour and more, the cold one not");
}

#[test]
fn the_counters_are_written_once_an_hour_and_only_when_something_was_asked_and_the_switch_is_off_by_default() {
    let m = Mock::start(|_, _| Say::Text(200, feed(&["v1.0.0"])));
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1_000_000_000);
    assert_eq!(b.events("watch.counts"), [json!({ "polls": 1, "quiet": 0, "changed": 1, "failed": 0 })]);
    w.pass(None, 1_000_000_000 + 59 * 60_000);
    assert_eq!(b.events("watch.counts").len(), 1, "not again within the hour");
    w.pass(None, 1_000_000_000 + 61 * 60_000);
    assert_eq!(b.events("watch.counts").len(), 2);
    let idle = bed("", true);
    idle.watcher(&m.url()).pass(None, 1_000_000_000);
    assert!(idle.events("watch.counts").is_empty(), "nothing asked, nothing counted");
    // the pack's own default: off
    let pack = Layer::new(LayerKind::Pack, "w", vec![file("settings/watch.toml", include_str!("../../../../packs/base/settings/watch.toml"))]);
    let (tables, _) = Tables::load(vec![pack]);
    assert!(!Settings::load(&tables).0.flag("watch.upstream", true), "off until the operator turns it on");
}

#[test]
fn a_gap_after_the_rest_ran_dry_reads_no_second_page_and_still_says_what_the_feed_holds() {
    let m = Mock::start(|seen: &Seen, n| {
        if seen.path.starts_with("/repos/") {
            return Say::Headed(200, vec![("x-ratelimit-remaining", "0".into())], "[]".into());
        }
        // each look finds ten entries none of which was seen
        let tags: Vec<String> = (0..10).map(|i| format!("v{}.0.0", n * 10 + i + 1)).rev().collect();
        Say::Text(200, feed(&tags.iter().map(String::as_str).collect::<Vec<_>>()))
    });
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    w.pass(None, 2);
    w.pass(None, 3);
    assert_eq!(count(&m, "/repos/"), 1, "GitHub said nothing was left: the floor holds");
    assert_eq!(b.events("watch.gap").len(), 2);
    assert!(b.events("watch.release").len() >= 20, "the feed's own entries are still said");
}

#[test]
fn the_floor_follows_what_github_says_is_left_and_an_answer_that_changes_nothing_writes_nothing() {
    let mut rich = Budget::default();
    rich.left(600);
    for t in 0..10 {
        assert!(rich.rest_ok("h", t));
        rich.used(t);
    }
    assert!(!rich.rest_ok("h", 11), "with plenty left it is still ten an hour");
    // the same feed again, with no validator to say so: a 200 whose content is the same is not a change
    let m = Mock::start(|_, _| Say::Text(200, feed(&["v1.0.0"])));
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = b.watcher(&m.url());
    w.pass(None, 1);
    w.pass(None, 2);
    assert_eq!((m.seen().len(), b.events("watch.seen").len(), b.events("watch.release").len()), (2, 1, 0));
}

#[test]
fn the_catalog_names_the_harnesses_by_their_own_feeds_and_binaries() {
    let pack = Layer::new(LayerKind::Pack, "c", vec![file("sources/catalog.toml", include_str!("../../../../packs/base/sources/catalog.toml"))]);
    let (tables, problems) = Tables::load(vec![pack]);
    assert!(problems.is_empty(), "{problems:?}");
    let rows: Vec<SourceRow> = tables.ids("source").iter().map(|id| tables.get::<SourceRow>(id).unwrap()).collect();
    assert_eq!(rows.len(), 12, "ten of the harnesses, and the models and the docs of Claude Code");
    for r in &rows {
        match r.adapter.as_str() {
            "atom" => assert!(r.url.as_deref().is_some_and(|u| u.starts_with("https://github.com/") && u.ends_with("/releases.atom")), "{}: {:?}", r.id, r.url),
            "probe" => assert_eq!(r.program.as_deref(), r.of.strip_prefix("connector:"), "{}", r.id),
            "models" | "page" => assert!(r.url.as_deref().is_some_and(|u| u.starts_with("https://")), "{}: {:?}", r.id, r.url),
            other => panic!("{}: {other}", r.id),
        }
        assert!(r.of.starts_with("connector:"), "{}", r.id);
    }
    assert_eq!(SourceRow::NAME, "source");
    // none is followed while its connector is not declared
    assert!(Source::load(&tables).0.is_empty());
}
