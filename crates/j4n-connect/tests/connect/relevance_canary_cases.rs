//! What a release says that matters (the classes, with no model), what a set of names did, and the
//! canary whose shape is compared with a good run's. The server is the loopback mock; the run is a script of ours; the Log, the folds
//! and the Table are the real ones.

use crate::mock::{Mock, Say};
use crate::watch_cases::{Bed, bed, bed_with, file, row};
use crate::world::{Scratch, script};
use j4n_connect::canary::{self, Verdict};
use j4n_connect::manifest::Connector;
use j4n_connect::relevance::{self, Change, Class, Uses};
use j4n_connect::watch::{Source, Watcher};
use j4n_core::front::Upstream;
use j4n_core::proposal::{Plant, Proposer};
use j4n_core::table::{Layer, LayerKind, Tables};
use j4n_core::write::Writer;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

fn uses() -> Uses {
    Uses::new(["--resume", "--model", "CLAUDE_CONFIG_DIR", "opus"])
}

#[test]
fn a_name_with_a_word_of_removal_breaks_a_name_alone_touches_added_alone_adds_and_the_rest_is_noise() {
    let u = uses();
    let class = |b: &str| relevance::classify(b, &u).0;
    assert_eq!(class("Removed the `--resume` flag"), Class::Breaks);
    assert_eq!(class("The --model flag is deprecated"), Class::Breaks);
    assert_eq!(class("CLAUDE_CONFIG_DIR is no longer read from the project"), Class::Breaks);
    assert_eq!(class("Fixed --resume for long sessions"), Class::Touches);
    assert_eq!(class("Added a /doctor command"), Class::Adds);
    assert_eq!(class("New: a thing"), Class::Adds);
    assert_eq!(class("The --resume flag is no longe here"), Class::Touches, "a word that is cut short is not the word");
    assert_eq!(class("Newly broken"), Class::Noise, "a word that only starts like one is not it");
    assert_eq!(class("Added --model aliases"), Class::Touches, "it names something J4n uses, so it is more than an addition");
    assert_eq!(class("Fixed a typo in the docs"), Class::Noise);
    assert_eq!(class("Removed the old banner"), Class::Noise, "a removal of something J4n does not use is noise");
    // a name is a whole word: neither a longer flag nor a part of one is the flag
    assert_eq!(class("Removed --resume-all and --models"), Class::Noise);
    assert_eq!(class("Removed pre--resume"), Class::Noise, "nor is a name that follows letters");
    assert_eq!(class("REMOVED THE --RESUME FLAG"), Class::Breaks, "case does not matter");
    assert_eq!(class("Set claude_config_dir"), Class::Touches, "a name in capitals is found in lower case");
    assert_eq!(relevance::classify("Fixed --resume and --model", &u).1, ["--model", "--resume"], "and it says which names");
    assert!(class("Opus is the default").ge(&Class::Touches) && class("Removed opus").eq(&Class::Breaks), "a model id is a name");
    for word in ["removed", "deprecated", "renamed", "no longer supported", "default", "breaking", "dropped", "replaced"] {
        assert_eq!(class(&format!("The --resume flag is {word}")), Class::Breaks, "{word}");
    }
}

#[test]
fn a_release_is_assessed_by_its_most_serious_bullet_and_a_security_word_is_not_for_the_digest() {
    let notes = "<ul><li>Fixed a typo</li><li>Added a new thing</li><li>Fixed --resume</li><li>Removed --model</li></ul><p>Fixes CVE-2026-1234</p>";
    let a = relevance::assess(notes, &uses());
    assert_eq!((a.class, a.counts), (Some(Class::Breaks), [1, 1, 1, 1]), "{a:?}");
    assert_eq!(a.items.iter().map(|i| i.0).collect::<Vec<_>>(), [Class::Breaks, Class::Touches, Class::Adds], "the serious first, noise not kept");
    assert!(a.security, "a CVE cannot wait for a digest");
    assert!(!relevance::assess("<li>Fixed a typo</li>", &uses()).security);
    for words in ["A security advisory was published", "Closes a vulnerability in the parser"] {
        assert!(relevance::assess(words, &uses()).security, "{words}");
    }
    for words in ["A security advisor was hired", "A vulnerabilit was found"] {
        assert!(!relevance::assess(words, &uses()).security, "{words}");
    }
    assert_eq!(relevance::assess("", &uses()).class, None, "no notes, no class");
    let many: String = (0..40).map(|i| format!("- Fixed --resume {i}\n")).collect();
    assert_eq!(relevance::assess(&many, &uses()).items.len(), relevance::KEEP, "a long list is cut");
}

#[test]
fn notes_in_html_and_in_markdown_are_read_as_bullets() {
    assert_eq!(relevance::bullets("<h2>Changes</h2><ul><li>First &amp; second</li><li>Third<br>more</li></ul>"), ["First & second", "Third"], "{:?}", relevance::plain("<li>Third<br>more</li>"));
    assert_eq!(relevance::bullets("## What's changed\n- one\n* two\n\nplain line\n"), ["one", "two"], "bullets, not headings or prose, when there are bullets");
    assert_eq!(relevance::bullets("Just a sentence.\nAnd another."), ["Just a sentence.", "And another."]);
    let long = format!("- {}", "x".repeat(900));
    assert!(relevance::bullets(&long)[0].chars().count() <= 301, "a bullet is clipped");
    assert!(relevance::bullets("").is_empty());
    assert_eq!(relevance::bullets("- one\n--resume stays"), ["one"], "a flag at the start of a line is not a bullet mark");
    assert_eq!(relevance::bullets("- --resume now needs a value"), ["--resume now needs a value"], "only the mark is taken off");
}

#[test]
fn what_a_connector_uses_is_generated_from_its_row_and_leaves_out_the_values_of_flags() {
    let c: Connector = toml::from_str(
        r#"id = "claude"
is = "agent"
dialect = "claude-stream"
program = "claude"
modes = { read = ["--permission-mode", "plan"], write = ["--permission-mode", "default"] }
models = ["haiku", "sonnet", "opus"]
env = { CLAUDE_CODE_FOO = "1" }
[signin]
argv = ["claude", "auth", "login", "--claudeai"]
env = { CLAUDE_CONFIG_DIR = "{account.home}" }
"#,
    )
    .unwrap();
    let u = Uses::of_connector(&c);
    let names: Vec<&str> = u.names().collect();
    for want in ["--permission-mode", "--claudeai", "login", "CLAUDE_CONFIG_DIR", "CLAUDE_CODE_FOO", "sonnet", "opus", "claude"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    for not in ["plan", "default", "auth"] {
        assert!(!names.contains(&not), "{not} is a value or a common word, not a name: {names:?}");
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// An Atom feed of GitHub's shape with the release notes in `<content type="html">`: (tag, notes as HTML), newest first.
fn feed_notes(entries: &[(&str, &str)]) -> String {
    let e: String = entries.iter().map(|(t, n)| format!("<entry><id>tag:github.com,2008:Repository/1/{t}</id><updated>2026-10-29T10:00:00Z</updated><link rel=\"alternate\" type=\"text/html\" href=\"https://github.com/o/r/releases/tag/{t}\"/><title>{t}</title><content type=\"html\">{}</content></entry>", esc(n))).collect();
    format!("<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><title>Releases</title>{e}</feed>")
}

fn with_uses(b: &Bed, m: &Mock) -> Watcher {
    b.watcher(&m.url()).with_uses(BTreeMap::from([("tool".to_string(), uses())]))
}

#[test]
fn a_release_event_carries_its_class_its_bullets_and_the_names_they_touch_and_a_quiet_one_says_noise() {
    let m = Mock::start(|_, n| match n {
        0 => Say::Text(200, feed_notes(&[("v1.0.0", "<li>Fixed a typo</li>")])),
        _ => Say::Text(200, feed_notes(&[("v1.2.0", "<li>Removed the --resume flag</li><li>Fixed a typo</li>"), ("v1.1.0", "<li>Fixed a typo</li>"), ("v1.0.0", "x")])),
    });
    let b = bed(&row("t", &format!("{}/o/r/releases.atom", m.url()), ""), true);
    let w = with_uses(&b, &m);
    w.pass(None, 1);
    let r = w.pass(None, 2);
    let got = b.events("watch.release");
    assert_eq!(got.iter().map(|g| (g["version"].as_str().unwrap(), g["class"].as_str().unwrap())).collect::<Vec<_>>(), [("1.1.0", "noise"), ("1.2.0", "breaks")], "{got:?}");
    assert_eq!(got[1]["items"], json!([{ "class": "breaks", "text": "Removed the --resume flag", "names": ["--resume"] }]));
    assert_eq!((got[1]["counts"].clone(), got[1]["security"].clone()), (json!([1, 0, 0, 1]), json!(false)));
    assert!(r.lines[0].contains("1.2.0 is out (breaks)") && !r.lines[0].contains("1.1.0 is out ("), "{r:?}");
}

#[test]
fn a_tool_or_a_model_that_is_gone_or_changed_breaks_with_no_model_and_one_that_is_added_only_adds() {
    let b = bed("", true);
    let w = b.watcher("http://127.0.0.1:1");
    let set = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>();
    // the first look only says where things stand
    assert!(w.observe("mcp:docs", &set(&[("search", "h1"), ("fetch", "h2")])).is_empty());
    assert_eq!((b.events("watch.set").len(), b.events("watch.diff").len()), (1, 0));
    // the same again writes nothing
    assert!(w.observe("mcp:docs", &set(&[("search", "h1"), ("fetch", "h2")])).is_empty());
    assert_eq!(b.events("watch.set").len(), 1, "unchanged: no write");
    // one gone and one changed
    let c = w.observe("mcp:docs", &set(&[("search", "h9")]));
    assert_eq!(c, [Change::Gone("fetch".into()), Change::Changed("search".into())]);
    let d = b.events("watch.diff");
    assert_eq!(d, [json!({ "of": "mcp:docs", "class": "breaks", "gone": ["fetch"], "changed": ["search"], "added": [], "lead": "fetch" })]);
    // only an addition
    w.observe("mcp:docs", &set(&[("search", "h9"), ("list", "h3")]));
    assert_eq!(b.events("watch.diff")[1]["class"], "adds");
    // a name gone and another added in one look: the more serious is the class
    w.observe("mcp:docs", &set(&[("list", "h3"), ("new", "h4")]));
    assert_eq!(b.events("watch.diff").pop().unwrap()["class"], "breaks");
    // a model row gone, in its own set
    w.observe("models:openrouter", &set(&[("a/model-1", ""), ("a/model-2", "")]));
    w.observe("models:openrouter", &set(&[("a/model-2", "")]));
    let last = b.events("watch.diff").pop().unwrap();
    assert_eq!((last["class"].clone(), last["gone"].clone()), (json!("breaks"), json!(["a/model-1"])));
    // and the sets are kept apart
    assert_eq!(b.events("watch.set").iter().filter(|e| e["of"] == "mcp:docs").count(), 4);
}

#[test]
fn the_shape_of_a_stream_is_the_kinds_and_the_names_and_types_of_fields_never_the_words() {
    let good = canary::shape_of("{\"type\":\"system\",\"session\":\"a\"}\nnot json\n{\"type\":\"result\",\"cost\":0.5,\"usage\":{\"in\":1,\"out\":[1,2]}}\n");
    assert!(good.contains("system.session:str") && good.contains("result.cost:num") && good.contains("result.usage.out[]:num"), "{good:?}");
    let other_words = canary::shape_of("{\"type\":\"system\",\"session\":\"zzz\"}\n{\"type\":\"result\",\"cost\":9,\"usage\":{\"in\":7,\"out\":[]}}");
    assert_eq!(canary::compare(&good, &other_words), Verdict::Differs { gone: vec!["result.usage.out[]:num".into()], new: vec![] }, "the elements an array has are part of its shape");
    assert!(canary::shape_of("{\"event\":\"x\",\"a\":1}\n{\"kind\":\"y\",\"b\":true}").is_superset(&["x.a:num".to_string(), "y.b:bool".to_string()].into()), "an event is named by its type, event or kind");
    let same = canary::shape_of("{\"type\":\"system\",\"session\":\"zzz\"}\n{\"type\":\"result\",\"cost\":9,\"usage\":{\"in\":7,\"out\":[3]}}");
    assert_eq!(canary::compare(&good, &same), Verdict::Same);
    let grown = canary::shape_of("{\"type\":\"system\",\"session\":\"a\",\"extra\":true}\n{\"type\":\"result\",\"cost\":1,\"usage\":{\"in\":1,\"out\":[1]}}");
    assert_eq!(canary::compare(&good, &grown), Verdict::Grew(vec!["system.extra:bool".into()]));
    let retyped = canary::shape_of("{\"type\":\"system\",\"session\":5}\n{\"type\":\"result\",\"cost\":1,\"usage\":{\"in\":1,\"out\":[1]}}");
    assert!(matches!(canary::compare(&good, &retyped), Verdict::Differs { gone, new } if gone == ["system.session:str"] && new == ["system.session:num"]));
}

/// The rows of a thing `tool`: its feed (with the canary `extra`) and the binary installed here.
fn tool(url: &str, extra: &str) -> String {
    format!("{}[[source]]\nid = \"t-bin\"\nof = \"tool\"\nadapter = \"probe\"\nprogram = \"tool-not-here\"\n", row("t", url, extra))
}

const GOOD: &str = "{\"type\":\"system\",\"session\":\"a\"}\n{\"type\":\"result\",\"cost\":0.5}\n";

/// A feed whose second answer has a release that touches `--resume`.
fn touching() -> Mock {
    Mock::start(|_, n| match n {
        0 => Say::Text(200, feed_notes(&[("v1.0.0", "<li>Fixed a typo</li>")])),
        _ => Say::Text(200, feed_notes(&[("v1.1.0", "<li>Fixed --resume</li>"), ("v1.0.0", "x")])),
    })
}

struct Home {
    home: Scratch,
    proposer: Arc<Proposer>,
}

/// A Proposer that writes the operator's folder, over the Table of `rows`.
fn proposer(b: &Bed, rows: &str) -> Home {
    let home = Scratch::new("ops");
    let (dir, rows) = (home.path().to_path_buf(), rows.to_string());
    let plant = Plant {
        load: Box::new(move || {
            let pack = Layer::new(LayerKind::Pack, "w", vec![file("sources/mine.toml", &format!("api = 1\n{rows}"))]);
            Tables::load(vec![pack, Layer::from_dir(LayerKind::User, "home", &dir).0]).0
        }),
        writer: Writer::new(Some(home.path().to_path_buf()), None),
    };
    let proposer = Arc::new(Proposer::new(b.log.clone(), b.proposals.clone(), b.changes.clone(), Arc::new(plant), Arc::new(|| 5_000)));
    Home { home, proposer }
}

#[test]
fn a_run_that_says_what_the_good_one_says_proposes_tested_and_a_changed_field_asks_at_once() {
    let run = Scratch::new("run");
    let good = run.put("good.jsonl", GOOD);
    // 1.1.0 says the same; 1.2.0 renamed a field
    let tool_run = script(&run, "run", "if [ \"$1\" = \"1.2.0\" ]; then printf '%s\\n' '{\"type\":\"system\",\"session\":\"b\"}' '{\"type\":\"result\",\"price\":0.5}'; else printf '%s\\n' '{\"type\":\"system\",\"session\":\"b\"}' '{\"type\":\"result\",\"cost\":0.7}'; fi");
    let m = Mock::start(|_, n| match n {
        0 => Say::Text(200, feed_notes(&[("v1.0.0", "<li>Fixed a typo</li>")])),
        1 => Say::Text(200, feed_notes(&[("v1.1.0", "<li>Fixed --resume</li>"), ("v1.0.0", "x")])),
        _ => Say::Text(200, feed_notes(&[("v1.2.0", "<li>Removed --resume</li>"), ("v1.1.0", "x"), ("v1.0.0", "x")])),
    });
    let extra = format!("canary = [\"{}\", \"{{version}}\"]\nrecord = \"{}\"", tool_run.display(), good.display());
    let rows = tool(&format!("{}/o/r/releases.atom", m.url()), &extra);
    let b = bed_with(&rows, true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
    let w = with_uses(&b, &m);
    let ops = proposer(&b, &rows);
    w.propose_to(ops.proposer.clone());
    w.pass(None, 1);
    let r = w.pass(None, 2);
    assert!(r.lines.iter().any(|l| l.contains("1.1.0 says the same")), "{r:?}");
    assert_eq!(b.events("watch.canary")[0]["outcome"], "same");
    ops.proposer.log_sync();
    let open = ops.proposer.proposals().open();
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].draft.title, "Mark tool 1.1.0 as tested");
    assert!(b.events("watch.shape").iter().any(|s| s["version"] == "1.1.0"), "the shape is kept");
    // the operator says yes: the row of the binary here says what was tested, and nothing else was written
    ops.proposer.accept(&open[0].id).unwrap();
    let file = std::fs::read_to_string(ops.home.join("sources/tested.toml")).unwrap();
    assert!(file.contains("id = \"t-bin\"") && file.contains("tested = \"1.1.0\""), "{file}");
    let (sources, problems) = Source::load(&Tables::load(vec![Layer::new(LayerKind::Pack, "w", vec![file_of(&rows)]), Layer::from_dir(LayerKind::User, "home", ops.home.path()).0]).0);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(sources.iter().find(|s| s.id == "t-bin").unwrap().tested.as_deref(), Some("1.1.0"));
    // 1.2.0 renamed a field: it is written as a difference to ask about at once, no shape is kept for it and nothing is proposed
    let r = w.pass(None, 3);
    assert!(r.lines.iter().any(|l| l.contains("1.2.0 says it differently (1 gone, 1 new)")), "{r:?}");
    let c = b.events("watch.canary");
    assert_eq!(c[1]["outcome"], "differs");
    assert_eq!(c[1]["more"], json!({ "gone": ["result.cost:num"], "new": ["result.price:num"] }));
    assert_eq!(c[1]["lead"], "result.cost:num", "the first path that is gone is the line to say");
    assert!(!b.events("watch.shape").iter().any(|s| s["version"] == "1.2.0"));
    ops.proposer.log_sync();
    assert_eq!(ops.proposer.proposals().open().len(), 0);
}

fn file_of(rows: &str) -> j4n_core::table::SourceFile {
    file("sources/mine.toml", &format!("api = 1\n{rows}"))
}

#[test]
fn the_new_release_goes_into_a_folder_of_its_own_that_is_removed_and_the_run_sees_none_of_the_operators_environment() {
    let run = Scratch::new("run");
    let seen = run.join("seen");
    let install = script(&run, "install", &format!("echo \"$1\" > \"$2/installed\"; echo \"$HOME\" > \"{}\"; echo \"${{CARGO_MANIFEST_DIR:-none}}\" >> \"{}\"", seen.display(), seen.display()));
    let go = script(&run, "go", "printf '{\"type\":\"result\",\"version\":\"%s\"}\\n' \"$(cat \"$1/installed\")\"");
    let good = run.put("good.jsonl", "{\"type\":\"result\",\"version\":\"x\"}\n");
    let m = touching();
    let extra = format!("install = [\"{}\", \"{{version}}\", \"{{dir}}\"]\ncanary = [\"{}\", \"{{dir}}\"]\nrecord = \"{}\"", install.display(), go.display(), good.display());
    let b = bed_with(&tool(&format!("{}/o/r/releases.atom", m.url()), &extra), true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
    let w = with_uses(&b, &m);
    w.pass(None, 1);
    w.pass(None, 2);
    assert_eq!(b.events("watch.canary")[0]["outcome"], "same", "{:?}", b.events("watch.canary"));
    let told = std::fs::read_to_string(&seen).unwrap();
    let mut lines = told.lines();
    let home = lines.next().unwrap();
    assert!(home.contains("j4n-canary-") && !std::path::Path::new(home).exists(), "the folder is its own and is gone: {home}");
    assert_eq!(lines.next(), Some("none"), "the operator's environment is not there");
}

#[test]
fn a_canary_runs_only_with_its_switch_on_a_row_that_names_it_and_a_release_that_touches_something() {
    let run = Scratch::new("run");
    let marker = run.join("ran");
    let go = script(&run, "go", &format!("echo x >> \"{}\"; printf '{{\"type\":\"a\"}}\\n'", marker.display()));
    let good = run.put("good.jsonl", "{\"type\":\"a\"}\n");
    let extra = format!("canary = [\"{}\"]\nrecord = \"{}\"", go.display(), good.display());
    // the switch is off by default
    let m = touching();
    let b = bed(&tool(&format!("{}/o/r/releases.atom", m.url()), &extra), true);
    let w = with_uses(&b, &m);
    w.pass(None, 1);
    w.pass(None, 2);
    assert!(!marker.exists(), "off: nothing ran");
    // on, but a release that is noise
    let quiet = Mock::start(|_, n| Say::Text(200, if n == 0 { feed_notes(&[("v1.0.0", "x")]) } else { feed_notes(&[("v1.1.0", "<li>Fixed a typo</li><li>Added a /doctor command</li>"), ("v1.0.0", "x")]) }));
    let on = bed_with(&tool(&format!("{}/o/r/releases.atom", quiet.url()), &extra), true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
    let w = with_uses(&on, &quiet);
    w.pass(None, 1);
    w.pass(None, 2);
    assert!(!marker.exists(), "a release that touches nothing (that only adds, or is noise) is not run");
    // on, a row with no canary
    let plain = touching();
    let none = bed_with(&tool(&format!("{}/o/r/releases.atom", plain.url()), ""), true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
    let w = with_uses(&none, &plain);
    w.pass(None, 1);
    w.pass(None, 2);
    assert!(!marker.exists() && none.events("watch.canary").is_empty());
    // and it is off in the pack
    let pack = Layer::new(LayerKind::Pack, "w", vec![file("settings/watch.toml", include_str!("../../../../packs/base/settings/watch.toml"))]);
    assert!(!j4n_core::setting::Settings::load(&Tables::load(vec![pack]).0).0.flag("watch.canary", true), "an install and a run are for the operator to switch on");
}

#[test]
fn a_run_that_fails_is_written_for_the_instant_item_and_a_first_run_with_no_good_one_keeps_its_shape_and_proposes_nothing() {
    let run = Scratch::new("run");
    let bad = script(&run, "bad", "exit 3");
    let slow_ok = script(&run, "ok", "printf '{\"type\":\"a\",\"n\":1}\\n'");
    for (prog, outcome) in [(&bad, "failed"), (&slow_ok, "baseline")] {
        let m = touching();
        let rows = tool(&format!("{}/o/r/releases.atom", m.url()), &format!("canary = [\"{}\"]", prog.display()));
        let b = bed_with(&rows, true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
        let ops = proposer(&b, &rows);
        let w = with_uses(&b, &m);
        w.propose_to(ops.proposer.clone());
        w.pass(None, 1);
        w.pass(None, 2);
        assert_eq!(b.events("watch.canary")[0]["outcome"], outcome);
        let lead = b.events("watch.canary")[0]["lead"].as_str().unwrap().to_string();
        assert_eq!(lead.contains("ended with"), outcome == "failed", "{outcome}: a failure says why in a line, a baseline says nothing: {lead:?}");
        ops.proposer.log_sync();
        assert_eq!(ops.proposer.proposals().open().len(), 0, "{outcome}: nothing to mark tested");
        assert_eq!(b.events("watch.shape").len(), usize::from(outcome == "baseline"));
    }
}

#[test]
fn the_standing_line_says_what_was_tested_and_a_later_run_is_held_to_the_shape_the_log_kept() {
    let run = Scratch::new("run");
    let go = script(&run, "go", "if [ -f \"$1\" ]; then printf '{\"type\":\"a\",\"n\":1,\"m\":2}\\n'; else printf '{\"type\":\"a\",\"n\":1}\\n'; fi");
    let flag = run.join("changed");
    let m = Mock::start(|_, n| match n {
        0 => Say::Text(200, feed_notes(&[("v1.0.0", "x")])),
        1 => Say::Text(200, feed_notes(&[("v1.1.0", "<li>Fixed --resume</li>"), ("v1.0.0", "x")])),
        _ => Say::Text(200, feed_notes(&[("v1.2.0", "<li>Fixed --resume</li>"), ("v1.1.0", "x"), ("v1.0.0", "x")])),
    });
    let rows = tool(&format!("{}/o/r/releases.atom", m.url()), &format!("canary = [\"{}\", \"{}\"]\n", go.display(), flag.display())).replace("program = \"tool-not-here\"", "program = \"tool-not-here\"\ntested = \"0.9.0\"");
    let b = bed_with(&rows, true, "[[setting]]\nid = \"watch.canary\"\nvalue = true\n");
    let w = with_uses(&b, &m);
    assert!(w.show().contains("tested 0.9.0"), "{}", w.show());
    w.pass(None, 1);
    w.pass(None, 2);
    assert_eq!(b.events("watch.canary")[0]["outcome"], "baseline");
    // the Log kept the first shape; a field added since is growth, and the next run is held to the new shape
    std::fs::write(&flag, "").unwrap();
    w.pass(None, 3);
    assert_eq!(b.events("watch.canary")[1]["outcome"], "grew");
    assert_eq!(b.events("watch.canary")[1]["more"], json!(["a.m:num"]));
}

#[test]
fn the_base_connectors_say_the_names_the_releases_are_read_against() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/connectors/agents.toml");
    let all: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let rows: Vec<Connector> = all["connector"].as_array().unwrap().iter().map(|c| c.clone().try_into().unwrap()).collect();
    let of = |id: &str| Uses::of_connector(rows.iter().find(|c| c.id == id).unwrap());
    let claude = of("claude").names().collect::<Vec<_>>().join(" ");
    for want in ["--permission-mode", "--claudeai", "CLAUDE_CONFIG_DIR", "sonnet"] {
        assert!(claude.contains(want), "{want} in {claude}");
    }
    let pi = of("pi").names().collect::<Vec<_>>().join(" ");
    assert!(pi.contains("--mode") && pi.contains("--session") && pi.contains("--tools") && !pi.contains("rpc"), "{pi}");
}
