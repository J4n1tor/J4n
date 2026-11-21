//! The operator's edit applies at once, is a Change in the Log with an undo and is
//! never a Grow row; a setting edited while J4n runs is what every reader holds at once; one wired at the start says it waits for the next.
//! The front, the proposer, the Writer and the reload are the real ones, wired as the binary wires them.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_core::value::Value;
use j4n_proto::{Down, Intent, Up};
use serde_json::{Value as Json, json};

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn events(w: &World, kind: &str) -> Vec<Json> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn notes(w: &World) -> Vec<String> {
    events(w, "chat.note").iter().filter_map(|n| n["text"].as_str().map(String::from)).collect()
}

fn set(serve: &mut Serve, kind: &str, id: &str, field: &str, value: Json) -> Vec<Down> {
    serve.up(Up::Intent { intent: Intent::Set { kind: kind.into(), id: id.into(), field: field.into(), value } }).0
}

fn now_of(w: &World, id: &str) -> Value {
    w.front.screen().setting_rows().into_iter().find(|r| r.pointer("/id") == Value::text(id)).map(|r| r.pointer("/now")).unwrap_or(Value::Missing)
}

#[test]
fn a_typed_look_is_an_edit_in_the_log_with_an_undo_and_no_grow_row() {
    let d = Dirs::new("edit-look");
    let world = open(&d);
    world.front.say("main", ":look frame gutter");
    let edits = events(&world, "edit.applied");
    assert_eq!(edits.len(), 1, "{edits:?}");
    assert_eq!(edits[0]["id"], "e1");
    assert!(events(&world, "proposal.made").is_empty(), "no Proposal for the operator's own edit");
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "nothing in Grow");
    assert_eq!(now_of(&world, "theme.frame"), Value::text("gutter"), "the Settings column reads the edit");
    assert!(notes(&world).iter().any(|n| n.contains("frame is gutter") && n.contains("`:undo e1`")), "{:?}", notes(&world));
    world.front.say("main", ":undo e1");
    assert_eq!(world.front.look().frame, "round");
    assert_eq!(now_of(&world, "theme.frame"), Value::text("round"));
    world.close();
}

#[test]
fn a_setting_set_from_a_screen_acts_at_once_and_undo_takes_it_back() {
    let d = Dirs::new("edit-set");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(world.front.cfg().session.word(), "auto");
    let mut downs = set(&mut serve, "setting", "session.mode", "value", json!("each"));
    assert_eq!(world.front.cfg().session.word(), "each", "the front reads it at once");
    assert_eq!(world.front.tasks().settings().settings.word("session.mode", "?"), "each", "and so do the engine's rules");
    downs.extend(serve.tick());
    assert_eq!(world.front.settings_rev(), 1, "the settings were read again once");
    assert!(!downs.iter().any(|d| matches!(d, Down::Tree { .. })), "no page reads `session.mode`: the tree is the same and is not sent again: {downs:?}");
    assert!(world.front.settings_waiting().is_empty(), "a live setting waits for nothing");
    // a word that is none of the row's is refused, and nothing is written
    set(&mut serve, "setting", "session.mode", "value", json!("sometimes"));
    assert!(notes(&world).iter().any(|n| n.contains("resume, auto, each")), "{:?}", notes(&world));
    assert_eq!(events(&world, "edit.applied").len(), 1);
    world.front.say("main", ":undo");
    assert_eq!(world.front.cfg().session.word(), "auto");
    world.close();
}

#[test]
fn a_setting_wired_at_the_start_says_it_waits_for_the_next_start() {
    let d = Dirs::new("edit-wait");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    set(&mut serve, "setting", "chats.on", "value", json!(true));
    assert_eq!(world.front.settings_waiting(), vec!["chats.on".to_string()], "the restart card names it");
    assert!(notes(&world).iter().any(|n| n.contains("⟳") && n.contains("⏎ restarts now")), "{:?}", notes(&world));
    let row = world.front.screen().setting_rows().into_iter().find(|r| r.pointer("/id") == Value::text("chats.on")).unwrap();
    assert_eq!(row.pointer("/waits"), Value::Bool(true));
    world.close();
}

#[test]
fn a_plan_price_is_set_in_place_and_an_unknown_kind_is_refused() {
    let d = Dirs::new("edit-plan");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    set(&mut serve, "plan", "max5", "price", json!(90));
    let edits = events(&world, "edit.applied");
    assert_eq!(edits.len(), 1, "{:?}", notes(&world));
    assert_eq!(world.front.screen().tables.raw("plan", "max5").and_then(|t| t.fields.get("price").cloned()), Some(toml::Value::String("$90".into())));
    set(&mut serve, "plan", "max5", "doc", json!("x"));
    set(&mut serve, "secret", "a", "value", json!("x"));
    let said = notes(&world);
    assert!(said.iter().any(|n| n.contains("price, period, assume")), "{said:?}");
    assert!(said.iter().any(|n| n.contains("nothing of kind `secret`")), "{said:?}");
    assert_eq!(events(&world, "edit.applied").len(), 1, "a refusal writes nothing");
    world.close();
}
