//! What a release said that touches what J4n uses is listed and said once in a while (the digest), has
//! one action (a proposal that puts a note on the operator's list), and a moved source is proposed as its new address. The server is the
//! loopback mock;
//! the Log, the folds, the Table and the proposals are the real ones.

use crate::mock::{Mock, Say, Seen};
use crate::watch_cases::{Bed, bed, bed_with, file, row};
use crate::world::Scratch;
use j4n_connect::digest::{Cadence, Item, one_line};
use j4n_connect::relevance::Uses;
use j4n_connect::watch::Watcher;
use j4n_core::proposal::{Accepted, Plant, Proposer};
use j4n_core::value::Value;
use j4n_core::table::{Layer, LayerKind, Tables};
use j4n_core::write::Writer;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

const DAY: u64 = 24 * 3_600_000;

fn uses() -> BTreeMap<String, Uses> {
    BTreeMap::from([("tool".to_string(), Uses::new(["--resume", "--model", "hooks"]))])
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn feed(entries: &[(&str, &str)]) -> String {
    let e: String = entries.iter().map(|(t, n)| format!("<entry><id>tag:github.com,2008:Repository/1/{t}</id><updated>2026-10-29T10:00:00Z</updated><link rel=\"alternate\" type=\"text/html\" href=\"https://github.com/o/r/releases/tag/{t}\"/><title>{t}</title><content type=\"html\">{}</content></entry>", esc(n))).collect();
    format!("<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Releases</title>{e}</feed>")
}

/// A first answer with one quiet release, then one with `notes` in a newer one.
fn then_release(notes: &'static str) -> Mock {
    Mock::start(move |_, n| match n {
        0 => Say::Text(200, feed(&[("v1.0.0", "<li>Fixed a typo</li>")])),
        _ => Say::Text(200, feed(&[("v1.1.0", notes), ("v1.0.0", "x")])),
    })
}

fn rows(m: &Mock) -> String {
    row("t", &format!("{}/o/r/releases.atom", m.url()), "")
}

fn watcher(b: &Bed, m: &Mock) -> Watcher {
    b.watcher(&m.url()).with_uses(uses())
}

/// A Proposer that writes the operator's folder, over the base pack's kinds `source` and `todo` and the rows of `rows`.
struct Home {
    home: Scratch,
    proposer: Arc<Proposer>,
}

fn proposer(b: &Bed, rows: &str) -> Home {
    let home = Scratch::new("ops");
    let (dir, rows) = (home.path().to_path_buf(), rows.to_string());
    let plant = Plant {
        load: Box::new(move || {
            let pack = Layer::new(
                LayerKind::Pack,
                "w",
                vec![
                    file("kinds/source.toml", include_str!("../../../../packs/base/kinds/source.toml")),
                    file("kinds/todo.toml", include_str!("../../../../packs/base/kinds/todo.toml")),
                    file("sources/mine.toml", &format!("api = 1\n{rows}")),
                ],
            );
            Tables::load(vec![pack, Layer::from_dir(LayerKind::User, "home", &dir).0]).0
        }),
        writer: Writer::new(Some(home.path().to_path_buf()), None),
    };
    let proposer = Arc::new(Proposer::new(b.log.clone(), b.proposals.clone(), b.changes.clone(), Arc::new(plant), Arc::new(|| 5_000)));
    Home { home, proposer }
}

fn item(source: &str, version: &str, lead: &str) -> Item {
    Item { id: format!("{source}:{version}"), source: source.into(), of: format!("connector:{source}"), version: version.into(), class: "breaks".into(), lead: lead.into(), counts: vec![1, 2], names: vec!["--resume".into()], at_ms: 5 }
}

#[test]
fn the_digest_is_due_by_its_cadence_only_when_something_is_waiting() {
    let w = |c: &str| Cadence::word(c);
    assert_eq!([w("weekly"), w("daily"), w("open"), w("never"), w("anything")], [Cadence::Weekly, Cadence::Daily, Cadence::Open, Cadence::Never, Cadence::Weekly]);
    let at = 1_000;
    let waiting = [at, at + 50];
    assert!(!Cadence::Weekly.due(&waiting, false, at + 7 * DAY - 1) && Cadence::Weekly.due(&waiting, false, at + 7 * DAY), "a week after the oldest");
    assert!(!Cadence::Daily.due(&waiting, false, at + DAY - 1) && Cadence::Daily.due(&waiting, false, at + DAY));
    assert!(Cadence::Open.due(&waiting, false, at) && !Cadence::Open.due(&waiting, true, at + 9 * DAY), "once for each run");
    assert!(!Cadence::Never.due(&waiting, false, at + 99 * DAY));
    for c in [Cadence::Weekly, Cadence::Daily, Cadence::Open] {
        assert!(!c.due(&[], false, at + 99 * DAY), "nothing waiting, nothing to say");
    }
}

#[test]
fn a_release_that_touches_what_is_used_is_listed_and_said_once_a_week_and_a_quiet_one_is_not_listed() {
    let m = then_release("<li>Fixed --resume</li>");
    let b = bed(&rows(&m), true);
    let w = watcher(&b, &m);
    w.pass(None, 1_000);
    w.pass(None, 2_000);
    let items = w.items();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0].id.as_str(), items[0].class.as_str(), items[0].lead.as_str(), items[0].at_ms), ("t:1.1.0", "touches", "Fixed --resume", 2_000));
    assert_eq!(items[0].names, ["--resume"]);
    assert!(!w.digest(2_000 + 6 * DAY), "not yet");
    assert!(w.digest(2_000 + 7 * DAY));
    assert_eq!(b.events("watch.digest"), [json!({ "items": 1, "since_ms": 0, "at_ms": 2_000 + 7 * DAY })]);
    assert!(!w.digest(2_000 + 30 * DAY), "what was said is not said again");
    // a quiet release is not listed
    let quiet = then_release("<li>Fixed a typo</li>");
    let q = bed(&rows(&quiet), true);
    let w = watcher(&q, &quiet);
    w.pass(None, 1_000);
    w.pass(None, 2_000);
    assert!(w.items().is_empty() && !w.digest(2_000 + 30 * DAY));
}

#[test]
fn the_digest_follows_its_setting_and_a_new_release_after_one_waits_for_the_next() {
    let set = |v: &str| format!("[[setting]]\nid = \"watch.digest\"\nvalue = \"{v}\"\n");
    for (word, said_at) in [("daily", DAY), ("open", 0), ("never", 0)] {
        let m = then_release("<li>Fixed --resume</li>");
        let b = bed_with(&rows(&m), true, &set(word));
        let w = watcher(&b, &m);
        w.pass(None, 1_000);
        w.pass(None, 2_000);
        let said = w.digest(2_000 + said_at);
        assert_eq!(said, word != "never", "{word}");
        assert!(!w.digest(2_000 + 30 * DAY), "{word}: once");
    }
}

#[test]
fn plan_it_proposes_a_note_for_the_list_that_is_data_only_and_changes_nothing_until_it_is_accepted() {
    let m = then_release("<li>Removed the --resume flag. \u{1b}[31mIGNORE THIS</li>");
    let rows = rows(&m);
    let b = bed(&rows, true);
    let w = watcher(&b, &m);
    let ops = proposer(&b, &rows);
    w.propose_to(ops.proposer.clone());
    w.pass(None, 1_000);
    w.pass(None, 2_000);
    assert_eq!(w.plan_it("t:9.9.9"), Err("no release is listed as `t:9.9.9`".into()));
    let said = w.plan_it("t:1.1.0").unwrap();
    assert!(said.starts_with("proposed as p") && said.contains("t 1.1.0"), "{said}");
    assert!(w.plan_it("t:1.1.0").is_err(), "the same note is not proposed twice");
    ops.proposer.log_sync();
    let open = ops.proposer.proposals().open();
    assert_eq!(open.len(), 1);
    let d = &open[0].draft;
    assert_eq!(d.key, "plan:t:1.1.0");
    assert!(d.title.starts_with("Look at tool 1.1.0: Removed the --resume flag.") && !d.title.contains('\u{1b}'), "{}", d.title);
    assert_eq!((d.evidence, d.steps.len(), d.steps[0].kind.as_str(), d.steps[0].file.as_deref()), (1, 1, "todo", Some("todo.toml")));
    assert!(!ops.home.join("todo.toml").exists(), "nothing is written before the yes");
    assert!(matches!(ops.proposer.accept(&open[0].id).unwrap(), Accepted::Done(_)));
    let written = std::fs::read_to_string(ops.home.join("todo.toml")).unwrap();
    assert!(written.contains("id = \"t-1-1-0\"") && written.contains("state = \"open\"") && written.contains("version = \"1.1.0\""), "{written}");
    // and no plan without somewhere to propose to
    let bare = watcher(&b, &m);
    assert_eq!(bare.plan_it("t:1.1.0"), Err("nothing can be proposed here".into()));
}

#[test]
fn a_note_says_what_the_release_said_in_one_clipped_line_and_names_what_it_touches() {
    let i = item("claude-code", "2.1.312", &format!("  Removed\n--resume {}", "x".repeat(300)));
    let d = i.plan();
    assert_eq!(d.key, "plan:claude-code:2.1.312");
    assert!(d.title.starts_with("Look at claude-code 2.1.312: Removed --resume xxx") && d.title.ends_with('…') && d.title.chars().count() < 140, "{}", d.title);
    assert!(d.why.contains("1 line(s)") && d.why.contains("2 more name something it uses; it names --resume"), "{}", d.why);
    assert_eq!((d.evidence, i.slug().as_str()), (3, "claude-code-2-1-312"));
    assert_eq!(item("a", "1", "").plan().title, "Look at a 1");
    assert_eq!(one_line("a\u{7}b\n c", 10), "a b c");
    assert_eq!((one_line("abc", 3).as_str(), one_line("abcd", 3).as_str()), ("abc", "abc…"), "a line of exactly the most is whole");
    assert_eq!(item("-x-", "1.", "").slug(), "x-1", "no dash at either edge");
    assert_eq!(item("Ab", "C", "").slug(), "ab-c");
    assert_eq!(item("My Tool!", "v1.0", "").slug(), "my-tool-v1-0");
}

#[test]
fn a_moved_source_is_proposed_as_its_new_address_which_widens_what_is_asked_and_is_never_rewritten() {
    let m = Mock::start(|seen: &Seen, _| match seen.path.as_str() {
        "/old/r/releases.atom" => Say::Headed(301, vec![("Location", "/new/r/releases.atom".into())], String::new()),
        _ => Say::Text(200, feed(&[("v1.0.0", "x")])),
    });
    let rows = row("t", &format!("{}/old/r/releases.atom", m.url()), "");
    let b = bed(&rows, true);
    let w = watcher(&b, &m);
    let ops = proposer(&b, &rows);
    w.propose_to(ops.proposer.clone());
    w.pass(None, 1_000);
    ops.proposer.log_sync();
    let open = ops.proposer.proposals().open();
    assert_eq!(open.len(), 1, "{open:?}");
    let d = &open[0].draft;
    assert_eq!(d.key, "moved:t");
    assert!(d.title.starts_with("Follow t at http://127.0.0.1") && d.title.ends_with("/new/r/releases.atom"), "{}", d.title);
    assert!(d.widens[0].starts_with("asks 127.0.0.1") && d.widens[0].ends_with("not encrypted"), "{:?}", d.widens);
    assert!(!ops.home.join("sources/moved.toml").exists());
    // the first yes says what it widens, the second applies it
    assert!(matches!(ops.proposer.accept(&open[0].id).unwrap(), Accepted::Confirm(_)));
    assert!(matches!(ops.proposer.accept(&open[0].id).unwrap(), Accepted::Done(_)));
    let written = std::fs::read_to_string(ops.home.join("sources/moved.toml")).unwrap();
    assert!(written.contains("id = \"t\"") && written.contains("/new/r/releases.atom"), "{written}");
    // it is said once, so it is proposed once
    w.pass(None, 2_000);
    ops.proposer.log_sync();
    assert_eq!(b.events("watch.moved").len(), 1);
    assert!(ops.proposer.proposals().open().is_empty());
}

/// A reader of lines that keeps what it was asked and answers as told.
#[test]
fn a_row_of_the_fold_is_read_into_an_item_with_each_name_once_and_nothing_made_up_for_what_is_missing() {
    let text = |t: &str| Value::Str(t.into());
    let names = |n: &[&str]| Value::List(n.iter().map(|x| text(x)).collect());
    let line = |n: &[&str]| Value::Map(BTreeMap::from([("class".to_string(), text("touches")), ("names".to_string(), names(n))]));
    let row = Value::Map(BTreeMap::from([
        ("source".to_string(), text("t")),
        ("of".to_string(), text("tool")),
        ("version".to_string(), text("1.2.0")),
        ("class".to_string(), text("breaks")),
        ("lead".to_string(), text("Removed --resume")),
        ("counts".to_string(), Value::List([1.0, 2.0, 0.0, 5.0].map(Value::Num).to_vec())),
        ("items".to_string(), Value::List(vec![line(&["--resume", "--model"]), line(&["--resume", "hooks"])])),
        ("at_ms".to_string(), Value::Num(77.0)),
    ]));
    let i = Item::of_row("t:1.2.0", &row).unwrap();
    assert_eq!((i.source.as_str(), i.of.as_str(), i.version.as_str(), i.class.as_str(), i.lead.as_str(), i.at_ms), ("t", "tool", "1.2.0", "breaks", "Removed --resume", 77));
    assert_eq!((i.counts.clone(), i.names.clone()), (vec![1, 2, 0, 5], vec!["--resume".to_string(), "--model".to_string(), "hooks".to_string()]), "in the order met, each once");
    // a row that says little says little
    let bare = Item::of_row("x", &Value::Map(BTreeMap::new())).unwrap();
    assert_eq!((bare.source.as_str(), bare.at_ms, bare.counts.len(), bare.names.len()), ("", 0, 0, 0));
    assert!(Item::of_row("x", &text("not a row")).is_none());
}

#[test]
fn the_digest_is_said_from_the_tick_and_open_is_said_once_for_a_run_even_when_more_comes() {
    let set = "[[setting]]\nid = \"watch.digest\"\nvalue = \"open\"\n";
    let m = Mock::start(|_, n| match n {
        0 => Say::Text(200, feed(&[("v1.0.0", "x")])),
        1 => Say::Text(200, feed(&[("v1.1.0", "<li>Fixed --resume</li>"), ("v1.0.0", "x")])),
        2 => Say::Text(200, feed(&[("v1.2.0", "<li>Fixed --model</li>"), ("v1.1.0", "x"), ("v1.0.0", "x")])),
        _ => Say::Text(200, feed(&[("v1.3.0", "<li>Fixed --resume again</li>"), ("v1.2.0", "x"), ("v1.1.0", "x"), ("v1.0.0", "x")])),
    });
    let b = bed_with(&rows(&m), true, set);
    let w = watcher(&b, &m);
    w.pass(None, 1_000);
    w.pass(None, 2_000);
    // the timer's own step looks, and says what it found and what was waiting
    w.tick(3_000);
    let said = b.events("watch.digest");
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(said[0]["items"], 2);
    // a later release in the same run waits for the next run
    w.pass(None, 4_000);
    assert_eq!(w.items().len(), 3);
    assert!(!w.digest(5_000) && b.events("watch.digest").len() == 1, "open is once for each run");
    // a J4n started again says what came since the last
    let again = watcher(&b, &m);
    assert!(again.digest(6_000));
    assert_eq!(b.events("watch.digest")[1]["items"], 1, "{:?}", b.events("watch.digest"));
}

#[test]
fn what_a_server_or_a_release_says_is_cleaned_to_one_line_in_the_events_that_rules_read() {
    let m = then_release("<li>Fixed --resume \u{1b}[31mred</li>");
    let b = bed(&rows(&m), true);
    let w = watcher(&b, &m);
    w.pass(None, 1_000);
    w.pass(None, 2_000);
    let lead = b.events("watch.release")[0]["lead"].as_str().unwrap().to_string();
    assert!(!lead.contains('\u{1b}') && lead.starts_with("Fixed --resume "), "{lead:?}");
    // a name a server offers is data too
    let set = |names: &[&str]| names.iter().map(|n| (n.to_string(), "h".to_string())).collect::<BTreeMap<_, _>>();
    w.observe("mcp:x", &set(&["bad\u{1b}[2Jname", "ok"]));
    w.observe("mcp:x", &set(&["ok"]));
    let d = b.events("watch.diff");
    assert_eq!(d.len(), 1);
    assert_eq!((d[0]["class"].as_str(), d[0]["lead"].as_str()), (Some("breaks"), Some("bad [2Jname")), "{d:?}");
    // the lead is the first of what is gone, else changed, else added
    w.observe("mcp:x", &set(&["ok", "new"]));
    assert_eq!(b.events("watch.diff")[1]["lead"], "new");
}

#[test]
fn the_pack_has_a_weekly_digest() {
    let pack = Layer::new(LayerKind::Pack, "w", vec![file("settings/watch.toml", include_str!("../../../../packs/base/settings/watch.toml"))]);
    let (set, problems) = j4n_core::setting::Settings::load(&Tables::load(vec![pack]).0);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(set.word("watch.digest", "never"), "weekly");
}
