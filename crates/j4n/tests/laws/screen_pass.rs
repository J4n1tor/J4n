//! The one switch has a row on the Tasks page and a line in Settings (what it turns on, the needs and the crew's ready
//! switch), and the Packs page says how many needs of a pack are unmet. The real front and Serve, with a `claude` that is a script.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::branch::Switch;
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Intent, Row, Up};
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn open(d: &Dirs) -> World {
    let p = d.dir.join("bin/claude");
    fs::write(&p, "#!/bin/sh\ncase \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\"}';; esac\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: p.display().to_string() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn until(what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(std::time::Instant::now() < end, "never saw {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn rows(serve: &mut Serve, bind: &str) -> Vec<Row> {
    let (downs, _) = serve.up(Up::Watch { bind: bind.into() });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

fn words(r: &Row) -> Vec<String> {
    r.cells.iter().map(|c| c.text.clone()).collect()
}

/// A home with the crew in J4n basic and its ready switch turned on: the word is `orchestrate` and the quick look and Main have no model, so a need is unmet.
fn orchestrating(tag: &str) -> (Dirs, World, Serve) {
    let d = Dirs::bare(tag);
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    world.front.say("main", ":pack ready on crew");
    until("the word written", || world.front.cfg().lanes == Switch::Orchestrate);
    until("the needs card", || world.front.need_asks().len() == 1);
    (d, world, serve)
}

#[test]
fn the_one_switch_is_the_first_row_of_the_tasks_page_while_it_is_not_single_and_says_what_it_turns_on_and_its_needs() {
    let (_d, world, mut serve) = orchestrating("screen-pass-switch");
    let tasks = rows(&mut serve, "tasks:all");
    let first = tasks.first().expect("the Tasks page has a row");
    assert_eq!(first.key, "switch:lanes", "{:?}", tasks.iter().map(words).collect::<Vec<_>>());
    let said = words(first);
    assert_eq!(said[1], "orchestrate");
    let n = world.front.needs_count("main");
    assert!(n > 0);
    assert_eq!(said[3], format!("{n} unmet · look crew envoys · crew ready"), "the needs first, then what the word turned on, then the crew's ready switch");
    assert!(first.actions.iter().any(|a| a.key == "enter" && matches!(&a.intent, Intent::Set { id, value, .. } if id == "lanes" && value == "single")), "⏎ turns it off: {:?}", first.actions);
    assert!(first.actions.iter().any(|a| a.key == "g" && matches!(&a.intent, Intent::Run { command, args } if command == "pack" && args == &["ready", "off", "crew"])), "g turns the crew's ready switch off: {:?}", first.actions);
    // the setting's own row says the same in its detail
    let settings = rows(&mut serve, "view:settings");
    let lanes = settings.iter().find(|r| r.key == "lanes").expect("the lanes row of Settings");
    assert!(lanes.detail.iter().any(|c| c.text == said[3]), "Settings says the same as the row: {:?}", lanes.detail.iter().map(|c| &c.text).collect::<Vec<_>>());
    assert!(settings.iter().filter(|r| r.key != "lanes").all(|r| r.detail.iter().all(|c| !c.text.contains("crew ready"))), "only the switch says it");
    world.close();
}

#[test]
fn a_fresh_single_has_no_switch_row_on_the_tasks_page() {
    let d = Dirs::bare("screen-pass-single");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    assert_eq!(world.front.lanes_now(), Switch::Single);
    assert!(rows(&mut serve, "tasks:all").iter().all(|r| r.key != "switch:lanes"));
    world.close();
}

#[test]
fn the_packs_row_says_how_many_needs_it_has_unmet_and_which() {
    let (_d, world, mut serve) = orchestrating("screen-pass-packs");
    let packs = rows(&mut serve, "packs:all");
    let base = packs.iter().find(|r| r.key == "base").expect("the base pack");
    let n = world.front.needs_count("main");
    assert!(n > 0);
    assert_eq!(base.cells[1].text, format!("on · {n} unmet"), "the state says the count: {:?}", words(base));
    assert!(base.detail.iter().any(|c| c.text.starts_with("needs ")), "its detail says which: {:?}", base.detail.iter().map(|c| &c.text).collect::<Vec<_>>());
    // the marks of a pack are one short column before what it adds, and a stability every pack shares is said in the detail, not on every row
    assert!(packs.iter().all(|r| r.cells.len() == 5 && !r.cells[3].text.contains("experimental") && r.detail.iter().any(|c| c.text == "experimental")), "{:?}", packs.iter().map(words).collect::<Vec<_>>());
    assert!(packs.iter().any(|r| r.cells[3].text.contains("widens")) && packs.iter().any(|r| r.cells[3].text == "ready"), "widening and ready are marks: {:?}", packs.iter().map(words).collect::<Vec<_>>());
    // a pack with nothing unmet says nothing about it
    assert!(packs.iter().filter(|r| r.key != "base").all(|r| !r.cells[1].text.contains("unmet")), "{:?}", packs.iter().map(words).collect::<Vec<_>>());
    world.close();
}
