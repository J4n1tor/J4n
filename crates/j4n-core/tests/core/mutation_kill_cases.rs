//! Kill-tests for survivors of the hand-made mutation pass over the pack catalog, views and commands, run
//! after the fact over the pack catalog, views, commands, the screen and the serve loop. Each test here failed against one
//! hand-injected bug and is kept so that bug cannot come back silently.

use j4n_core::pack::{Catalog, Info, Origin};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::front::command::Commands;
use j4n_core::front::screen::Screen;
use j4n_core::query::Coll;
use j4n_core::setting::Settings;
use j4n_core::value::Value;
use j4n_core::view::{Sources, Views, outer};
use j4n_proto::{Down, Tone, Up};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::front_cases::{Fx, fx_screened};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::front::Said;
use j4n_core::front::serve::Serve;

fn manifest(id: &str) -> String {
    format!("api = 1\nid = \"{id}\"\nversion = \"0.1.0\"\nsummary = \"the pack {id}\"\nstability = \"experimental\"\n")
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("j4n-b13-mutation-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &std::path::Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

// ---- pack.rs: install() must refuse a pack whose files hold a refused problem (kill: pk15) ----

#[test]
fn installing_a_pack_whose_files_are_refused_by_the_table_is_an_error_and_nothing_is_copied() {
    let from = dir("from");
    put(&from, "pack.toml", &manifest("oversize"));
    // one .toml file over the 1 MiB layer limit (`table::MAX_FILE`) is a Refused problem, not a Warn
    put(&from, "views/big.toml", &"#".repeat((1 << 20) + 1));
    let into = dir("into");
    let mut c = Catalog::new(Vec::new());
    assert!(c.install(&from, &into).is_err(), "a pack with a refused file must not install");
    assert!(!into.join("oversize").exists(), "nothing was copied");
}

// ---- view.rs: a value stored as Value::Missing reads as blank, not the word "missing" (kill: vw1) ----

struct Fake(Vec<Value>);

impl Sources for Fake {
    fn rows(&self, _coll: &Coll) -> Vec<Value> {
        self.0.clone()
    }
}

fn view_tables(toml: &str) -> Tables {
    let files = vec![SourceFile { path: "views/t.toml".into(), text: toml.into() }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "t", files)]);
    assert!(found.is_empty(), "{found:?}");
    t
}

#[test]
fn a_row_whose_id_field_is_explicitly_missing_falls_through_to_its_place_not_the_word_missing() {
    const V: &str = r#"api = 1
[[view]]
id = "v"
from = { from = "tasks" }
cells = ["{n}"]
"#;
    let (views, found) = Views::load(&view_tables(V));
    assert!(found.is_empty(), "{found:?}");
    let v = views.get("v").unwrap();
    let row = Value::Map(BTreeMap::from([("id".to_string(), Value::Missing), ("n".to_string(), Value::num(1.0))]));
    let rows = v.rows(&Fake(vec![row]), &outer(Value::Map(BTreeMap::new())));
    assert_eq!(rows[0].key, "0", "a blank id falls through to the row's place, not the literal word `missing`");
}

// ---- command.rs: an `open` command's tag lookup must find the layer that set `open` (kill: cm5) ----

#[test]
fn an_open_command_from_a_tagged_pack_carries_that_packs_tag() {
    const TOML: &str = r#"api = 1
[[page]]
id = "somewhere"
parent = "home"
title = "Somewhere"
order = 1
present = "stack"
field = "log"
bind = "log:all"
[[command]]
id = "go-there"
open = "somewhere"
"#;
    let files = vec![SourceFile { path: "pages/p.toml".into(), text: TOML.into() }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "mypack", files)]);
    assert!(found.is_empty(), "{found:?}");
    let tags = BTreeMap::from([("mypack".to_string(), "experimental".to_string())]);
    let (commands, found) = Commands::load(&t, &tags);
    assert!(found.is_empty(), "{found:?}");
    let c = commands.all().iter().find(|c| c.id == "go-there").unwrap();
    assert_eq!(c.tag, "experimental", "an `open` command must be tagged by the pack that set its `open` field");
}

// ---- screen.rs: pack_rows() must mark a widening pack, and pack_values() must tell "off" from "left out" (kill: sc2, sc3) ----

fn info(id: &str, on: bool, note: &str, widens: bool) -> Info {
    Info { id: id.into(), version: "0.1.0".into(), summary: "s".into(), use_when: String::new(), tag: "", origin: Origin::Builtin, on, note: note.into(), adds: "nothing".into(), widens, pin: String::new(), words: Vec::new(), ready: None, keyed: Vec::new() }
}

#[test]
fn pack_rows_marks_a_pack_that_widens_what_j4n_may_do() {
    let mut screen = Screen::builtin();
    screen.packs = vec![info("wide", true, "", true), info("plain", true, "", false)];
    let rows = screen.pack_rows();
    let wide = rows.iter().find(|r| r.key == "wide").unwrap();
    assert!(wide.cells.iter().any(|c| c.text == "widens"), "a widening pack's row must say `widens`: {:?}", wide.cells);
    let plain = rows.iter().find(|r| r.key == "plain").unwrap();
    assert!(!plain.cells.iter().any(|c| c.text == "widens"), "a pack that adds nothing widening must not say `widens`: {:?}", plain.cells);
}

#[test]
fn pack_values_tells_off_from_left_out_by_whether_there_is_a_note() {
    let mut screen = Screen::builtin();
    screen.packs = vec![info("quiet", false, "", false), info("sorry", false, "it needs `x`, which is not on", false)];
    let values = screen.pack_values();
    let state_of = |id: &str| match values.iter().find(|v| matches!(v, Value::Map(m) if m.get("id") == Some(&Value::text(id)))) {
        Some(Value::Map(m)) => match m.get("state") {
            Some(Value::Str(s)) => s.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert_eq!(state_of("quiet"), "off", "off with no note is off, not left out");
    assert_eq!(state_of("sorry"), "left out", "off with a note is left out, not plain off");
}

#[test]
fn pack_rows_tone_matches_off_vs_left_out_too() {
    let mut screen = Screen::builtin();
    screen.packs = vec![info("quiet", false, "", false), info("sorry", false, "it needs `x`, which is not on", false)];
    let rows = screen.pack_rows();
    let row = |id: &str| rows.iter().find(|r| r.key == id).unwrap();
    assert_eq!(row("quiet").tone, Tone::Dim, "off is dim");
    assert_eq!(row("sorry").tone, Tone::Bad, "left out (asked for but refused) is bad");
}

// ---- serve.rs: a watched view is refreshed on a non-run.* event too, not only on run.* ones (kill: se4) ----

fn custom_tasks_view() -> Screen {
    const TOML: &str = r#"api = 1
[[view]]
id = "mytasks"
from = { from = "tasks" }
cells = ["{id}"]
"#;
    let files = vec![SourceFile { path: "views/t.toml".into(), text: TOML.into() }];
    let (t, found) = Tables::load(vec![Layer::new(LayerKind::Pack, "t", files)]);
    assert!(found.is_empty(), "{found:?}");
    let settings = Settings::load(&t).0;
    Screen::load(t, &settings, Vec::new(), &BTreeMap::new()).0
}

fn serve(f: &Fx) -> Serve {
    Serve::new(f.front.clone())
}

fn watch(s: &mut Serve, bind: &str) -> Vec<Down> {
    let (downs, quit) = s.up(Up::Watch { bind: bind.into() });
    assert!(!quit, "{downs:?}");
    downs
}

#[test]
fn a_watched_view_over_tasks_is_sent_again_when_a_task_event_comes_not_only_on_run_events() {
    let f = fx_screened(vec![Scripted::new("alpha", vec![vec![Beat::Session("s".into()), say("a"), end("a")]])], custom_tasks_view(), |_| {});
    let mut s = serve(&f);
    let before = watch(&mut s, "view:mytasks");
    assert!(matches!(&before[..], [Down::Rows { rows, .. }] if rows.is_empty()), "no task yet: {before:?}");
    let task = match f.say("go") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    // only the events whose kind is not `run.*` (a `thread.` or `task.` one): the view must be refreshed on these too, not only on
    // the doer's own `run.*` events
    let kinds: Vec<String> = f.rig.log.read(0).map(|e| e.kind.clone()).collect();
    assert!(kinds.iter().any(|k| !k.starts_with("run.")), "the task produced no non-run event to test with: {kinds:?}");
    let downs: Vec<Down> = f.rig.log.read(0).filter(|e| !e.kind.starts_with("run.")).flat_map(|e| s.ev(&e)).collect();
    let got = downs.iter().any(|d| matches!(d, Down::Rows { bind, rows, .. } if bind == "view:mytasks" && !rows.is_empty()));
    assert!(got, "the view must be sent again once a non-run.* event (task./thread.) is heard, from these kinds {kinds:?}: {downs:?}");
}
