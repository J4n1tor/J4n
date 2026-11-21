//! Packs that are ready to go: a pack's `[ready]` names the settings it turns on; enabling the pack with ready on writes them in the same edit as the enable, runs the
//! needs check (one card for what the mode lacks), and one `:undo` takes all of it back; turning ready off or the pack off takes back what ready wrote unless the
//! operator changed it since; a pack that is on in J4n basic does not fire it by itself. The real front, proposer and Serve, with a `claude` that is a script.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::branch::Switch;
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Up};
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn claude(d: &Dirs) -> String {
    let p = d.dir.join("bin/claude");
    fs::write(&p, "#!/bin/sh\ncase \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}';; esac\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: claude(d) };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// A home where the crew pack is off, as an operator who left it out has it (J4n basic loads it).
fn crew_off(tag: &str) -> Dirs {
    let d = Dirs::new(tag);
    d.committed();
    fs::write(d.j4n_home().join("crew-off.toml"), "api = 1\n[[enable]]\nid = \"crew\"\non = false\n").unwrap();
    d
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(std::time::Instant::now() < end, "never saw {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn say(w: &World, line: &str) {
    w.front.say("main", line);
}

fn asks(serve: &mut Serve) -> usize {
    let (downs, _) = serve.up(Up::Watch { bind: "asks:open".into() });
    downs.into_iter().filter_map(|d| if let Down::Asks { open } = d { Some(open) } else { None }).flatten().count()
}

fn what_was_edited(w: &World) -> Vec<String> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == "edit.applied").flat_map(|e| e.data["what"].as_array().cloned().unwrap_or_default()).filter_map(|v| v.as_str().map(String::from)).collect()
}

fn row_keys(w: &World, id: &str) -> Vec<String> {
    w.front.screen().pack_rows().into_iter().find(|r| r.key == id).map(|r| r.actions.into_iter().map(|a| a.key).collect()).unwrap_or_default()
}

#[test]
fn enabling_a_pack_with_ready_writes_its_settings_in_the_same_edit_shows_the_needs_card_and_one_undo_takes_it_all_back() {
    let d = crew_off("ready-enable");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(world.front.lanes_now(), Switch::Single);
    assert!(!world.front.screen().packs.iter().any(|p| p.id == "crew" && p.on));
    assert_eq!(asks(&mut serve), 0, "nothing is asked while the mode is off");
    say(&world, ":pack on crew");
    until("the word written", || world.front.cfg().lanes == Switch::Orchestrate);
    let said = what_was_edited(&world);
    assert!(said.iter().any(|w| w == "enable crew") && said.iter().any(|w| w == "setting lanes"), "one edit holds both: {said:?}");
    until("the needs card", || world.front.need_asks().len() == 1);
    assert_eq!(world.front.need_asks()[0].question, "lanes needs the quick look and Main");
    say(&world, ":undo");
    until("the word taken back", || world.front.cfg().lanes == Switch::Single);
    let screen = world.front.screen();
    assert!(screen.tables.layer_entry(j4n_core::change::Where::User, "setting", "lanes").is_none(), "no value of lanes is left in the operator's layer");
    world.close();
}

#[test]
fn ready_off_before_enabling_writes_nothing_and_off_takes_back_what_ready_wrote_unless_it_was_changed_since() {
    let d = crew_off("ready-off");
    let world = open(&d);
    say(&world, ":pack ready off crew");
    until("ready off said", || what_was_edited(&world).iter().any(|w| w == "enable crew"));
    say(&world, ":pack on crew");
    until("the pack on", || what_was_edited(&world).iter().filter(|w| *w == "enable crew").count() == 2);
    assert_eq!(world.front.lanes_now(), Switch::Single, "ready was off, so enabling wrote no setting");
    world.close();
    drop(world);
    // ready on: the pack off takes the word back
    let d = crew_off("ready-back");
    let world = open(&d);
    say(&world, ":pack on crew");
    until("the word written", || world.front.cfg().lanes == Switch::Orchestrate);
    say(&world, ":pack off crew");
    until("the word taken back", || world.front.cfg().lanes == Switch::Single);
    world.close();
    drop(world);
    // changed since: the operator's own word stays
    let d = crew_off("ready-kept");
    let world = open(&d);
    say(&world, ":pack on crew");
    until("the word written", || world.front.cfg().lanes == Switch::Orchestrate);
    say(&world, ":lanes wide");
    until("wide", || world.front.cfg().lanes == Switch::Wide);
    say(&world, ":pack off crew");
    until("the pack off said", || what_was_edited(&world).iter().filter(|w| *w == "enable crew").count() == 2);
    assert_eq!(world.front.cfg().lanes, Switch::Wide, "what the operator changed since is not taken back");
    world.close();
}

#[test]
fn a_pack_that_is_on_in_basic_does_not_fire_ready_by_itself_and_its_switch_writes_and_takes_back_the_word() {
    let d = Dirs::bare("ready-basic");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert!(world.front.screen().packs.iter().any(|p| p.id == "crew" && p.on), "the crew is in J4n basic");
    assert_eq!((world.front.cfg().lanes, asks(&mut serve)), (Switch::Single, 0), "a fresh J4n stays single and asks nothing");
    assert!(row_keys(&world, "crew").iter().any(|k| k == "g"), "the row has the ready-to-go key: {:?}", row_keys(&world, "crew"));
    assert!(row_keys(&world, "lanes").iter().all(|k| k != "g"), "a pack with no `[ready]` has none");
    let label = |w: &World| w.front.screen().pack_rows().into_iter().find(|r| r.key == "crew").unwrap().actions.into_iter().find(|a| a.key == "g").unwrap().label;
    assert_eq!(label(&world), "ready to go on");
    say(&world, ":pack ready on crew");
    until("the word written", || world.front.cfg().lanes == Switch::Orchestrate);
    assert_eq!(label(&world), "ready to go off", "the row says it is on when the word holds");
    until("the needs card", || world.front.need_asks().len() == 1);
    say(&world, ":pack ready off crew");
    until("the word taken back", || world.front.cfg().lanes == Switch::Single);
    say(&world, ":pack ready off crew");
    until("refused", || world.front.log().read(0).any(|e| e.kind == "chat.note" && e.data["text"].as_str().is_some_and(|t| t.contains("already"))));
    world.close();
}

#[test]
fn every_ready_row_of_a_built_in_pack_names_settings_that_exist_and_take_the_value() {
    let d = Dirs::bare("ready-law");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/full.toml"), "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n").unwrap();
    let world = open(&d);
    let screen = world.front.screen();
    let with: Vec<&str> = screen.packs.iter().filter(|p| p.ready.is_some()).map(|p| p.id.as_str()).collect();
    assert!(with.contains(&"crew"), "the crew has a `[ready]`: {with:?}");
    for p in screen.packs.iter() {
        if let Some(r) = &p.ready {
            let found = r.problems(&p.id, &screen.tables);
            assert!(found.is_empty(), "{}: {found:?}", p.id);
        }
    }
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    world.close();
}
