//! Settings. The basic switches are the rows of Settings › Features and every switch is a row of All, from the settings' own types; `space` and
//! `⏎` send the operator's edit (`Intent::Set`) and `u` takes it back; every other setting row carries its editor; Settings ›
//! Gates lists the grants that count and `x` takes one back. The front, the proposer, the Writer and the reload are the real ones.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::front::serve::Serve;
use j4n_core::gate::Action;
use j4n_core::gate::grant::{here_id, write_here};
use j4n_core::log::New;
use j4n_core::task::Mode;
use j4n_proto::{Down, Intent, Row, Up};
use serde_json::{Value as Json, json};

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn notes(w: &World) -> Vec<String> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == "chat.note").filter_map(|e| e.data["text"].as_str().map(String::from)).collect()
}

/// The rows of a view, as the screen is sent them.
fn rows(serve: &mut Serve, view: &str) -> Vec<Row> {
    let (downs, _) = serve.up(Up::Watch { bind: format!("view:{view}") });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

fn row(serve: &mut Serve, view: &str, key: &str) -> Row {
    rows(serve, view).into_iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key} in {view}"))
}

fn act(r: &Row, key: &str) -> Option<Intent> {
    r.actions.iter().find(|a| a.key == key).map(|a| a.intent.clone())
}

fn send(serve: &mut Serve, intent: Intent) {
    serve.up(Up::Intent { intent });
}

#[test]
fn the_basic_switches_are_the_features_rows_and_every_switch_is_flipped_in_all_from_the_settings_types() {
    let d = Dirs::new("settings-features");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    // Features lists the settings marked `basic` and no other: the switches an operator changes, each with its one plain line
    let features = rows(&mut serve, "settings-features");
    let keys: Vec<&str> = features.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, ["approval.inside", "chats.on", "check.after", "cost.show", "lane.doer", "lane.model", "lanes", "packs.full", "tabs.all", "theme.marks", "theme.palette"]);
    for r in &features {
        let brief = &r.cells[3].text;
        assert!(!brief.is_empty() && brief.chars().count() <= 100 && !brief.contains('`'), "{}: one plain line: {brief:?}", r.key);
    }
    // every other switch is a row of All, from the settings' own types, flipped there with `space`
    let all = rows(&mut serve, "settings");
    let keys: Vec<&str> = all.iter().map(|r| r.key.as_str()).collect();
    for id in ["chats.on", "look.keys_row", "look.generate", "session.mode", "weigh.on", "librarian.on", "retry.auto", "watch.upstream", "lanes", "work.apply"] {
        let r = all.iter().find(|r| r.key == id).unwrap_or_else(|| panic!("{id} is no row of All: {keys:?}"));
        let edit = &r.data.as_ref().unwrap_or_else(|| panic!("{id} carries no editor"))["edit"];
        assert!(matches!(edit["kind"].as_str(), Some("bool" | "enum")), "{id}: {edit}");
        assert!(matches!(act(r, "enter"), Some(Intent::Set { value: Json::Null, .. })), "{id}: ⏎ opens or flips");
        assert!(matches!(act(r, " "), Some(Intent::Set { .. })), "{id}: space flips");
    }
    // a word or a switch flips with space; the agent and its model are words typed with ⏎
    for r in features.iter().filter(|r| !matches!(r.key.as_str(), "theme.palette" | "lane.doer" | "lane.model")) {
        assert!(matches!(act(r, " "), Some(Intent::Set { .. })), "{}: space flips", r.key);
    }
    let usd = all.iter().find(|r| r.key == "plan.usd_line").unwrap();
    assert!(act(usd, " ").is_none() && act(all.iter().find(|r| r.key == "cool.max").unwrap(), " ").is_none(), "a number is no switch: it has no space");
    let session = row(&mut serve, "settings", "session.mode");
    assert_eq!(session.data.as_ref().unwrap()["edit"]["options"], json!(["resume", "auto", "each"]));
    assert_eq!(act(&session, " "), Some(Intent::Set { kind: "setting".into(), id: "session.mode".into(), field: "value".into(), value: json!("each") }), "space takes the next word");
    // the editor of every other kind: a duration, a number in dollars, a number with its bounds
    let lp = row(&mut serve, "settings-loop", "cool.max");
    assert_eq!(lp.data.as_ref().unwrap()["edit"]["kind"], "dur");
    let all = row(&mut serve, "settings", "plan.usd_line");
    assert_eq!(all.data.as_ref().unwrap()["edit"]["kind"], "usd");
    let look = row(&mut serve, "settings-look", "theme.palette");
    assert_eq!(look.data.as_ref().unwrap()["edit"]["kind"], "enum");
    assert!(act(&look, "n").is_some() && act(&look, "enter").is_some(), "the Look row keeps `n` and is changed with ⏎");
    world.close();
}

#[test]
fn space_flips_a_switch_u_takes_it_back_and_one_of_the_next_start_says_so() {
    let d = Dirs::new("settings-flip");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let keys = row(&mut serve, "settings", "look.keys_row");
    assert!(act(&keys, "u").is_none(), "nothing to take back yet");
    send(&mut serve, act(&keys, " ").unwrap());
    let keys = row(&mut serve, "settings", "look.keys_row");
    assert_eq!(keys.data.as_ref().unwrap()["edit"]["value"], json!(false), "{:?}", notes(&world));
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "nothing in Grow");
    let Some(Intent::Say { text, .. }) = act(&keys, "u") else { panic!("no undo on the row: {keys:?}") };
    assert_eq!(text, ":undo e1");
    send(&mut serve, Intent::Say { text, with: Vec::new() });
    let keys = row(&mut serve, "settings", "look.keys_row");
    assert_eq!(keys.data.as_ref().unwrap()["edit"]["value"], json!(true));
    assert!(act(&keys, "u").is_none(), "taken back: no more undo");
    // chats: wired at the start, so the row says it waits and `r` restarts
    let chats = row(&mut serve, "settings", "chats.on");
    assert!(act(&chats, "r").is_none());
    send(&mut serve, act(&chats, " ").unwrap());
    let chats = row(&mut serve, "settings", "chats.on");
    let edit = &chats.data.as_ref().unwrap()["edit"];
    assert_eq!((edit["waits"].as_bool(), edit["next_start"].as_bool(), edit["start"].as_bool()), (Some(true), Some(true), Some(false)), "{edit}");
    assert_eq!(act(&chats, "r"), Some(Intent::Say { text: ":restart".into(), with: Vec::new() }));
    world.close();
}

#[test]
fn a_grant_is_a_row_of_gates_and_x_takes_it_back() {
    let d = Dirs::new("settings-grants");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let once = json!({ "id": "g0000000001", "scope": "once", "verb": "shell", "target": "make build", "session": "old" });
    world.front.log().append(New::of("grant", Actor::Operator, once)).unwrap();
    let old = json!({ "id": "g0000000002", "scope": "session", "verb": "shell", "target": "make test", "session": "old" });
    world.front.log().append(New::of("grant", Actor::Operator, old)).unwrap();
    world.front.log().sync();
    let gates = rows(&mut serve, "settings-gates");
    let g = gates.iter().find(|r| r.key == "g0000000001").unwrap_or_else(|| panic!("the grant is no row: {gates:?}"));
    assert_eq!(g.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["g0000000001", "shell", "allow", "once", "make build"]);
    assert!(!gates.iter().any(|r| r.key == "g0000000002"), "a grant of a session that is over counts no more");
    assert!(gates.iter().any(|r| r.key != "g0000000001" && act(r, "x").is_none()), "a gate of the packs has no take-back");
    let Some(Intent::Say { text, .. }) = act(g, "x") else { panic!("no x: {g:?}") };
    assert_eq!(text, ":grant back g0000000001");
    send(&mut serve, Intent::Say { text, with: Vec::new() });
    assert!(!rows(&mut serve, "settings-gates").iter().any(|r| r.key == "g0000000001"), "{:?}", notes(&world));
    world.front.log().sync();
    assert!(world.front.log().read(0).any(|e| e.kind == "grant.revoked" && e.by == Actor::Operator && e.data["id"] == "g0000000001"));
    world.close();
}

#[test]
fn always_here_taken_back_takes_the_row_of_the_operators_gates_with_an_undo() {
    let d = Dirs::new("settings-here");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let action = Action::shell(Actor::Doer("claude".into()), "cargo build");
    write_here(&d.j4n_home(), &d.project(), &action, &Actor::Operator, 1, &mut j4n_core::write::NoJournal).unwrap();
    let here = json!({ "id": "g00000000aa", "scope": "here", "verb": "shell", "target": "cargo build", "session": "s" });
    world.front.log().append(New::of("grant", Actor::Operator, here)).unwrap();
    world.front.log().sync();
    let g = row(&mut serve, "settings-gates", "g00000000aa");
    assert_eq!(g.cells[3].text, "always here");
    world.front.say("main", ":grant back g00000000aa");
    let gate = here_id(&action);
    assert!(world.front.screen().tables.raw("gate", &gate).is_none(), "the row of always here is gone: {:?}", notes(&world));
    let said = notes(&world);
    let undo = said.iter().find_map(|n| n.split("`:undo ").nth(1).and_then(|r| r.split('`').next())).unwrap_or_else(|| panic!("no undo said: {said:?}")).to_string();
    world.front.say("main", &format!(":undo {undo}"));
    assert!(world.front.screen().tables.raw("gate", &gate).is_some(), "the undo puts the row back: {:?}", notes(&world));
    world.close();
}

/// under `packs.full` each loaded pack's own feature switch is on, Settings › Features lists it with `full` in the column `by`, the operator's own row wins over it, and a
/// basic home has none of it.
#[test]
fn packs_full_turns_the_feature_switches_on_and_says_so_in_features_and_the_operators_row_wins() {
    let on = ["kit.mode", "lanes", "offload.on", "parts.mode", "qc.on", "reflexes.on", "shape.on"];
    let d = Dirs::new("settings-full");
    std::fs::write(d.j4n_home().join("settings/full.toml"), "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n").unwrap();
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let set = world.front.screen().set.clone();
    assert_eq!((set.pointer("/kit/mode"), set.pointer("/lanes"), set.pointer("/qc/on"), set.pointer("/shape/on"), set.pointer("/reflexes/on")), (j4n_core::value::Value::text("on"), j4n_core::value::Value::text("auto"), j4n_core::value::Value::Bool(true), j4n_core::value::Value::Bool(true), j4n_core::value::Value::Bool(true)));
    let features = rows(&mut serve, "settings-features");
    for id in on {
        let r = features.iter().find(|r| r.key == id).unwrap_or_else(|| panic!("{id} is no row of Features"));
        assert!(r.cells.get(2).is_some_and(|c| c.text == "full"), "{id}: the column `by` says full: {:?}", r.cells);
    }
    // what a pack that acts outside the project keeps: not a row of Features, and not turned on
    for id in ["contribute.mode", "inbox.mode", "kit.judge", "qc.jev", "reflexes.judge", "librarian.on", "weigh.on"] {
        assert!(!features.iter().any(|r| r.key == id && r.cells.get(2).is_some_and(|c| c.text == "full")), "{id} is the operator's");
    }
    assert_eq!(world.front.screen().set.pointer("/kit/judge"), j4n_core::value::Value::text("off"));
    world.close();
    // the operator's own row for it wins, and the row says nothing of full
    let d = Dirs::new("settings-mine");
    std::fs::write(d.j4n_home().join("settings/full.toml"), "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n").unwrap();
    std::fs::write(d.j4n_home().join("settings/mine.toml"), "api = 1\n[[setting]]\nid = \"kit.mode\"\nvalue = \"off\"\n").unwrap();
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(world.front.screen().set.pointer("/kit/mode"), j4n_core::value::Value::text("off"));
    let features = rows(&mut serve, "settings-features");
    assert!(!features.iter().any(|r| r.key == "kit.mode" && r.cells.get(2).is_some_and(|c| c.text == "full")), "kit.mode is the operator's now");
    assert!(features.iter().any(|r| r.key == "qc.on" && r.cells.get(2).is_some_and(|c| c.text == "full")), "the others are still full's");
    world.close();
    // a basic home: none of them is on, and Features has no row `by` full
    let b = Dirs::new("settings-basic");
    let world = open(&b);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(world.front.screen().set.pointer("/kit/mode"), j4n_core::value::Value::text("off"));
    assert!(!rows(&mut serve, "settings-features").iter().any(|r| r.cells.get(2).is_some_and(|c| c.text == "full")));
    world.close();
}

#[test]
fn tab_completes_a_commands_word_from_the_core_and_complete_on_is_flipped_live() {
    let d = Dirs::new("settings-complete");
    let world = open(&d);
    let mut op = crate::screen_drive::Operator::sit(&world);
    // a phrase command, which the palette does not list, is the core's word for the slot of a command
    op.press(j4n_tui::headless::typed(":sw", false));
    op.until_app("the ghost of `:switch`", |a| a.ghost().as_deref() == Some("itch"));
    op.press(vec![j4n_tui::headless::ctrl('u')]);
    // the screen asks for the words of `:pack` as it is typed, and the ghost comes when the answer does
    op.press(j4n_tui::headless::typed(":pack o", false));
    op.until_app("the ghost of `:pack on`", |a| a.ghost().as_deref() == Some("n"));
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::Tab)]);
    assert_eq!(op.app.line.buf, ":pack on");
    // the switch is a row of All, read live: off, the screen is told at once and completes nothing; on again, the same
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let flip = |serve: &mut Serve| {
        let space = act(&row(serve, "settings", "complete.on"), " ").unwrap();
        send(serve, space);
    };
    flip(&mut serve);
    op.until_app("completion off", |a| !a.complete_on());
    flip(&mut serve);
    op.until_app("completion on again", |a| a.complete_on());
    world.close();
}

/// `keys.hold`: `auto` by default, so the screen asks a terminal that speaks the kitty protocol for every key as a code (⌥ held is seen);
/// `off` keeps the two flags the keys always had. The flags are asked before the screen is up, so the row says it waits for the next start.
#[test]
fn keys_hold_off_keeps_todays_two_flags_and_its_row_says_it_waits_for_the_next_start() {
    let d = Dirs::new("settings-keys-hold");
    let world = open(&d);
    assert!(j4n::keys_hold(&world.front), "auto by default");
    assert_eq!(j4n_tui::term::key_flags(true).bits(), 0b1111, "auto: disambiguate, event types, alternate keys, every key as a code");
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let hold = row(&mut serve, "settings", "keys.hold");
    let edit = &hold.data.as_ref().expect("an editor")["edit"];
    assert_eq!((edit["kind"].as_str(), &edit["options"]), (Some("enum"), &json!(["auto", "off"])), "{edit}");
    assert_eq!(act(&hold, " "), Some(Intent::Set { kind: "setting".into(), id: "keys.hold".into(), field: "value".into(), value: json!("off") }));
    send(&mut serve, act(&hold, " ").unwrap());
    let hold = row(&mut serve, "settings", "keys.hold");
    let edit = &hold.data.as_ref().unwrap()["edit"];
    assert_eq!((edit["value"].as_str(), edit["next_start"].as_bool()), (Some("off"), Some(true)), "{edit}");
    assert_eq!(act(&hold, "r"), Some(Intent::Say { text: ":restart".into(), with: Vec::new() }), "the restart card follows");
    // what the screen reads as it starts
    assert!(!j4n::keys_hold(&world.front), "off: {:?}", notes(&world));
    assert_eq!(j4n_tui::term::key_flags(false).bits(), 0b0011, "off: disambiguate and event types, as before");
    world.close();
}
