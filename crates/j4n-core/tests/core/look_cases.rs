//! The look file. Its layers merge in order and the operator's entry wins; what it does not
//! cover is counted, with a sample masked by the redactor; a pass asks the ask point `look` (here a scripted decider, $0) for entries
//! for the misses only, checks the answer and logs `look.made`, or refuses it, keeps the old look and logs `look.refused`; at most once
//! an hour, never while a task works, never with `look.generate` off.

use crate::front_cases::{Fx, fx_over, fx_screened};
use crate::scripted::{Beat, Scripted, end};
use crate::world::{Scratch, open_log};
use j4n_core::answer::Paid;
use j4n_core::change::Actor;
use j4n_core::front::lookfile::{self, HOUR, LookRow, check, fill, json};
use j4n_core::front::screen::Screen;
use j4n_core::front::{Cfg, Said};
use j4n_core::log::{Ev, New};
use j4n_core::setting::Settings;
use j4n_core::table::{Kind, Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::State;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

const BUILTIN: &str = include_str!("../../../../packs/base/look/look.toml");

fn layer(kind: LayerKind, name: &str, text: &str) -> Layer {
    Layer::new(kind, name, vec![SourceFile { path: "look.toml".into(), text: text.into() }])
}

/// The look of the base pack, as the screen is sent it.
fn builtin_json() -> Json {
    let (tables, found) = Tables::load(vec![layer(LayerKind::Pack, "base", BUILTIN)]);
    assert!(found.is_empty(), "{found:?}");
    json(&tables.list::<LookRow>().unwrap())
}

/// A front whose pass keeps what it makes in `made`, with a decider that picks the first of `picks` that is offered.
fn front(picks: Option<&'static str>, made: PathBuf) -> Fx {
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    fx_over(project, home, log, clock, Vec::new(), picks, move |c: &mut Cfg| c.look_made = Some(made))
}

/// Write `kind` with `data` to the Log and show it to the front `n` times; the event.
fn see(f: &Fx, kind: &str, data: Json, n: usize) -> Ev {
    let ev = f.rig.log.append(New::of(kind, Actor::Core, data)).unwrap();
    (0..n).for_each(|_| f.front.look_see(&ev));
    ev
}

#[test]
fn the_built_in_look_file_is_the_one_the_screen_is_sent_and_covers_every_kind_j4n_writes() {
    let got = builtin_json();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/look.json");
    let text = format!("{got:#}\n");
    if std::env::var_os("J4N_BLESS").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "fixtures/look.json is the base pack's look file as sent (J4N_BLESS=1 to write it)");
    // the kinds the code and the packs write (a sample of each family): none falls to the bare look
    let families: Vec<&String> = got["family"].as_object().unwrap().keys().filter(|p| !p.is_empty()).collect();
    for kind in [
        "task.started", "thread.bound", "ask.step", "answerer.state", "model.listed", "trust.labelled", "account.signin", "gate.verdict",
        "probe.result", "bench.case", "tool.run", "run.shell", "run.tool_call", "run.reasoning", "run.message", "run.error", "chat.note",
        "plan.item", "rule.fired", "timer.fired", "log.repaired", "cost.charged", "spend.cap", "guard.alert", "grant.revoked", "watch.seen",
        "shape.card", "draft", "report.made", "inbox.item", "ledger.report", "proposal.made", "change.recorded", "contribute.found",
        "pack.on", "look.made", "connector.made", "branch.started", "lane.ended", "lanes.set", "intake.decided", "brief.rendered",
        "flow.step.ended", "parts.run", "qc.mark", "map.answer", "box.unavailable", "complete.ranked",
    ] {
        assert!(families.iter().any(|p| kind.starts_with(p.as_str())), "`{kind}` has a family in the built-in look");
    }
    assert_eq!(got["family"]["run.tool_"], json!({ "glyph": "tool", "role": "fetch" }), "the look table, as data");
    assert_eq!((got["summary"]["run.shell"].as_str(), got["field"]["_ms"].as_str(), got["tone"]["revoked"].as_str()), (Some("{data.cmd}"), Some("duration"), Some("bad")));
}

#[test]
fn the_layers_merge_in_order_and_the_operators_entry_wins() {
    let made = "api = 1\n[[look]]\nid = \"family/task.\"\nglyph = \"done\"\nrole = \"ok\"\n[[look]]\nid = \"family/zz.\"\nglyph = \"tool\"\nrole = \"fetch\"\n";
    let mine = "api = 1\n[[look]]\nid = \"family/task.\"\nglyph = \"ask\"\nrole = \"warn\"\n";
    let pinned = std::env::temp_dir().join(format!("j4n-look-{}", std::process::id()));
    std::fs::write(&pinned, made).unwrap();
    let layers = vec![layer(LayerKind::Pack, "base", BUILTIN), lookfile::layer(&pinned), layer(LayerKind::User, "home", mine)];
    std::fs::remove_file(&pinned).unwrap();
    let (tables, found) = Tables::load(layers);
    assert!(found.is_empty(), "{found:?}");
    let got = json(&tables.list::<LookRow>().unwrap());
    assert_eq!(got["family"]["task."], json!({ "glyph": "ask", "role": "warn" }), "the operator's entry wins over the made one and the pack's");
    assert_eq!(got["family"]["zz."], json!({ "glyph": "tool", "role": "fetch" }), "a made entry fills what the pack has not");
    assert_eq!(got["family"]["run."], json!({ "glyph": "run", "role": "run" }), "the pack's stands where nobody said another");
    assert_eq!(lookfile::layer(&PathBuf::from("/nowhere/look.generated.toml")).files.len(), 0, "no file: an empty layer");
}

#[test]
fn an_unknown_kind_and_a_number_with_no_renderer_are_counted_as_misses_and_known_ones_are_not() {
    let f = front(None, Scratch::new("made").join("look.generated.toml"));
    see(&f, "task.started", json!({ "doer": "alpha", "took_ms": 12, "seq": 3 }), 1);
    assert!(f.front.look_misses().is_empty(), "a kind with a family and numbers with renderers are no miss: {:?}", f.front.look_misses());
    see(&f, "zz.new", json!({ "name": "one", "wait_secs": 12 }), 3);
    let m = f.front.look_misses();
    assert_eq!(m.keys().collect::<Vec<_>>(), ["field:wait_secs", "kind:zz.new"]);
    assert_eq!((m["kind:zz.new"].n, m["kind:zz.new"].samples.len()), (3, 2), "counted each time, two samples kept");
    assert_eq!(m["kind:zz.new"].samples[0]["data"]["name"], "one");
    assert_eq!(f.front.look_due().unwrap_err(), "6 misses, fewer than 12, and none a day old");
    f.rig.clock.advance(lookfile::DAY);
    assert_eq!(f.front.look_due(), Ok(()), "a day with any misses makes a pass due");
}

#[test]
fn a_pass_says_what_its_asks_cost_when_the_decider_reports_it() {
    let made = Scratch::new("made");
    let f = front(Some("look=tool|fetch,{data.name},duration"), made.join("cache/look.generated.toml"));
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.0004), tokens_in: 120, tokens_out: 7 });
    see(&f, "zz.new", json!({ "name": "one", "wait_ms_x": 12 }), 12);
    let data = f.front.look_pass().expect("a pass was due");
    assert_eq!(data["asked"], 3, "{data:#}");
    assert!((data["usd"].as_f64().expect("a number") - 0.0012).abs() < 1e-12, "three asks at $0.0004: {data:#}");
    assert_eq!((data["tokens_in"].as_u64(), data["tokens_out"].as_u64()), (Some(360), Some(21)));
    let logged = f.rig.of_kind("look.made");
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].data["usd"], data["usd"], "what is in the Log is what the pass returned");
    for step in f.rig.of_kind("ask.step") {
        assert_eq!((step.data["usd"].as_f64(), step.data["tokens_in"].as_u64()), (Some(0.0004), Some(120)), "{step:?}");
    }
    for ev in f.rig.of_kind("ask.answered") {
        assert!(ev.data["usd"].as_f64().is_some(), "an answer a decider gave says what its walk cost: {ev:?}");
    }
}

#[test]
fn a_refused_pass_says_what_it_cost_too() {
    let made = Scratch::new("made");
    // a line wider than 64 for the second sample: the entries are refused whole, but the asks were paid for
    let f = front(Some("look=tool|fetch,{data.name}"), made.join("look.generated.toml"));
    *f.pays.lock().unwrap() = Some(Paid { usd: Some(0.001), tokens_in: 10, tokens_out: 1 });
    see(&f, "zz.new", json!({ "name": "short" }), 1);
    see(&f, "zz.new", json!({ "name": "a long name ".repeat(8) }), 11);
    let data = f.front.look_pass().expect("a pass was due");
    let logged = f.rig.of_kind("look.refused");
    assert_eq!(logged.len(), 1, "{data:#}");
    assert!((logged[0].data["usd"].as_f64().expect("a number") - 0.002).abs() < 1e-12, "two asks at $0.001: {data:#}");
    assert_eq!((logged[0].data["tokens_in"].as_u64(), logged[0].data["tokens_out"].as_u64()), (Some(20), Some(2)));
}

#[test]
fn a_yes_or_no_with_no_renderer_is_a_miss_as_a_number_is_and_is_asked_with_tick_and_onoff() {
    let f = front(Some("look=onoff,tick,text"), Scratch::new("made").join("look.generated.toml"));
    see(&f, "task.started", json!({ "doer": "alpha", "resumed": false, "ok": true, "passed": true, "told": true }), 1);
    assert!(f.front.look_misses().is_empty(), "the yes or no fields the built-in look names are no miss: {:?}", f.front.look_misses());
    see(&f, "task.started", json!({ "doer": "alpha", "zz_flag": true, "zz_wait": 3, "zz_name": "x" }), 12);
    let m = f.front.look_misses();
    assert_eq!(m.keys().collect::<Vec<_>>(), ["field:zz_flag", "field:zz_wait"], "a yes or no is counted as a number is, a string is not");
    assert_eq!(m["field:zz_flag"].samples[0]["data"]["zz_flag"], true);
    let data = f.front.look_pass().expect("a pass was due");
    assert_eq!(data["made"], json!(["field/zz_flag", "field/zz_wait"]), "{data:#}");
    let got = json(&f.front.look_rows());
    assert_eq!(got["field"]["zz_flag"], "onoff", "a yes or no is offered `onoff`, `tick` and `text`, and the decider's first is taken");
    assert_eq!(got["field"]["zz_wait"], "text", "a number is not offered `onoff` or `tick`: its first offered renderer that the decider names is `text`");
    assert!(f.front.look_misses().is_empty(), "what was asked is no longer counted");
}

#[test]
fn a_pass_with_a_scripted_decider_fills_the_misses_and_logs_look_made() {
    let made = Scratch::new("made");
    let path = made.join("cache/look.generated.toml");
    let f = front(Some("look=tool|fetch,{data.name},duration"), path.clone());
    see(&f, "zz.new", json!({ "name": "one", "wait_ms_x": 12 }), 12);
    let data = f.front.look_pass().expect("a pass was due");
    assert_eq!(data["made"], json!(["family/zz.", "field/wait_ms_x", "summary/zz.new"]), "{data:#}");
    assert_eq!((data["asked"].as_u64(), data["left"].as_array().map(Vec::len)), (Some(3), Some(0)));
    assert_eq!(data["by"], json!(["decider:jev", "decider:jev", "decider:jev"]));
    assert!(data["usd"].is_null(), "a decider that does not report its cost is unknown, not $0 and not a guess");
    assert_eq!(data["tokens_in"], 0, "{data:#}");
    let steps = f.rig.of_kind("ask.step");
    assert!(steps.len() == 3 && steps.iter().all(|e| e.data["usd"].is_null() && e.data.as_object().unwrap().contains_key("usd")), "each step says its cost is unknown: {steps:?}");
    assert_eq!(f.count("look.made"), 1);
    let got = json(&f.front.look_rows());
    assert_eq!((got["family"]["zz."].clone(), got["summary"]["zz.new"].clone(), got["field"]["wait_ms_x"].clone()), (json!({ "glyph": "tool", "role": "fetch" }), json!("{data.name}"), json!("duration")));
    // the file holds the entries and loads as a layer of its own
    let (tables, found) = Tables::load(vec![lookfile::layer(&path)]);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(tables.ids(LookRow::NAME), ["family/zz.", "field/wait_ms_x", "summary/zz.new"]);
    assert!(f.front.look_misses().is_empty(), "what was asked is no longer counted");
    see(&f, "zz.other", json!({}), 1);
    assert!(f.front.look_misses().is_empty(), "the made family covers its kinds now");
}

#[test]
fn a_bad_answer_is_refused_and_the_old_look_kept() {
    let none = BTreeMap::new();
    let unknown = check("[[look]]\nid = \"family/zz.\"\nglyph = \"tool\"\nrole = \"neon\"\n", &none).unwrap_err();
    assert!(unknown.contains("neon"), "a role that does not exist: {unknown}");
    let slot = check("[[look]]\nid = \"family/zz.\"\nglyph = \"rocket\"\nrole = \"ok\"\n", &none).unwrap_err();
    assert!(slot.contains("rocket"), "a glyph slot that does not exist: {slot}");
    let show = check("[[look]]\nid = \"field/x\"\nshow = \"sparkle\"\n", &none).unwrap_err();
    assert!(show.contains("sparkle"), "a renderer that does not exist: {show}");
    assert!(check("[[look]\nid = ", &none).is_err(), "not TOML");
    assert!(check("[[look]]\nid = \"family/zz.\"\nglyph = \"tool\"\n", &none).unwrap_err().contains("not a whole entry"));
    let wide = BTreeMap::from([("zz.new".to_string(), vec![json!({ "data": { "name": "x".repeat(90) } })])]);
    assert!(check("[[look]]\nid = \"summary/zz.new\"\ntext = \"{data.name}\"\n", &wide).unwrap_err().contains("wider than 64"));
    assert_eq!(fill("{data.tool} {data.cmd} {data.none}", &json!({ "data": { "tool": "Bash", "cmd": "ls\nmore" } })), "Bash ls");

    // a pass whose line fits the first sample and not the second: refused whole, nothing kept or written
    let made = Scratch::new("made");
    let path = made.join("look.generated.toml");
    let f = front(Some("look=tool|fetch,{data.name}"), path.clone());
    see(&f, "zz.new", json!({ "name": "short" }), 1);
    see(&f, "zz.new", json!({ "name": "a long name ".repeat(8) }), 11);
    let before = json(&f.front.look_rows());
    let data = f.front.look_pass().expect("a pass was due");
    assert!(data["why"].as_str().unwrap().contains("wider than 64"), "{data:#}");
    assert_eq!((f.count("look.refused"), f.count("look.made")), (1, 0));
    assert_eq!(json(&f.front.look_rows()), before, "the old look is kept");
    assert!(!path.exists(), "the file is not written");
}

#[test]
fn the_hourly_limit_and_a_working_task_hold_the_pass() {
    let doer = Scripted::new("alpha", vec![vec![Beat::Session("s".into()), Beat::Hold, end("done")]]);
    let (project, home) = (Scratch::new("proj"), Scratch::new("home"));
    let (log, clock) = open_log(&project);
    let path = Scratch::new("made").join("look.generated.toml");
    let f = fx_over(project, home, log, clock, vec![doer], Some("look=tool|fetch"), move |c: &mut Cfg| c.look_made = Some(path));
    see(&f, "zz.one", json!({}), 12);
    assert!(f.front.look_pass().is_some());
    see(&f, "yy.two", json!({}), 12);
    assert_eq!(f.front.look_due().unwrap_err(), "the last pass was less than an hour ago");
    assert!(f.front.look_pass().is_none());
    f.rig.clock.advance(HOUR);
    let Said::Started { task } = f.say("start") else { panic!("{}", f.rig.dump()) };
    f.rig.until(&task, "running", |r| r.state == State::Running);
    assert_eq!(f.front.look_due().unwrap_err(), "a task works", "never while a turn runs");
    assert!(f.rig.tasks.ctl(&task, j4n_core::task::Ctl::Abort, Actor::Operator).is_ok());
    f.rig.finished(&task);
    assert_eq!(f.front.look_due(), Ok(()));
    assert_eq!(f.front.look_pass().unwrap()["made"], json!(["family/yy."]));
}

#[test]
fn the_switch_off_counts_nothing_and_makes_no_pass() {
    let files = vec![
        SourceFile { path: "look/look.toml".into(), text: BUILTIN.into() },
        SourceFile { path: "settings/look.toml".into(), text: include_str!("../../../../packs/base/settings/look.toml").into() },
    ];
    let off = layer(LayerKind::User, "home", "api = 1\n[[setting]]\nid = \"look.generate\"\nvalue = false\n");
    let (tables, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files), off]);
    assert!(found.is_empty(), "{found:?}");
    let (settings, _) = Settings::load(&tables);
    let screen = Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0;
    let path = Scratch::new("made").join("look.generated.toml");
    let f = fx_screened(Vec::new(), screen, move |c: &mut Cfg| c.look_made = Some(path));
    see(&f, "zz.new", json!({ "n_x": 1 }), 30);
    f.rig.clock.advance(lookfile::DAY);
    assert!(f.front.look_misses().is_empty());
    assert_eq!(f.front.look_due().unwrap_err(), "look.generate is off");
    assert!(f.front.look_pass().is_none());
    assert_eq!(f.count("look.made") + f.count("look.refused"), 0);
}

#[test]
fn the_redactor_masks_a_secret_in_a_sample() {
    let f = front(None, Scratch::new("made").join("look.generated.toml"));
    let secret = "hunter2-very-secret-value-0042";
    let ev = f.rig.log.append(New::of("zz.secret", Actor::Core, json!({ "line": format!("login with {secret} now") }))).unwrap();
    assert!(ev.data.to_string().contains(secret), "the Log did not know it yet when it was written");
    assert!(f.rig.log.redactor().register("pass", secret));
    f.front.look_see(&ev);
    let sample = f.front.look_misses()["kind:zz.secret"].samples[0].to_string();
    assert!(!sample.contains(secret) && sample.contains("login with"), "masked before any model sees it: {sample}");
}

#[test]
fn the_screen_is_sent_the_look_file_when_it_watches_it_and_again_after_a_pass() {
    use j4n_core::front::serve::Serve;
    use j4n_proto::{Down, LOOK_FILE, Up};
    let f = front(Some("look=tool|fetch"), Scratch::new("made").join("look.generated.toml"));
    let mut s = Serve::new(f.front.clone());
    let value = |downs: Vec<Down>| downs.into_iter().find_map(|d| match d {
        Down::Value { bind, value, .. } if bind == LOOK_FILE => Some(value),
        _ => None,
    });
    let first = value(s.up(Up::Watch { bind: LOOK_FILE.into() }).0).expect("the look file is sent");
    assert_eq!(first, builtin_json(), "the base pack's look, as the base screen has it");
    // the Log's events are counted as they come; enough of them make a pass, off the screen's thread
    let ev = f.rig.log.append(New::of("zz.new", Actor::Core, json!({}))).unwrap();
    (0..12).for_each(|_| drop(s.ev(&ev)));
    let made = f.rig.until_event("the pass", |e| e.kind == "look.made");
    let again = value(s.ev(&made)).expect("sent again after a pass");
    assert_eq!(again["family"]["zz."], json!({ "glyph": "tool", "role": "fetch" }));
}
