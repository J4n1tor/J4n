//! J4n basic and J4n full: a folder that says nothing loads six packs (the four and `lanes` and `crew`, which act only under the word `orchestrate`); `packs.full` loads every pack built in; a
//! folder that already uses a feature of a pack keeps the pack, and the doctor says why; the operator's own `enable` rows win over all of it.
//! The packs are read from the real catalog and the real wiring (`wire::open`), in folders of the test's own.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::doctor::{Facts, report};
use j4n_core::pack::enables;
use j4n_core::table::{Layer, LayerKind};
use j4n_core::task::Mode;
use std::collections::BTreeSet;
use std::fs;

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// The packs a world loaded, and every pack it was told of.
fn packs(w: &World) -> (BTreeSet<String>, BTreeSet<String>) {
    let all = w.front.screen().packs.clone();
    (all.iter().filter(|p| p.on).map(|p| p.id.clone()).collect(), all.iter().map(|p| p.id.clone()).collect())
}

fn ids(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

/// A home with a file in it: `Dirs::bare` has none, so what a pack is on for is what the test wrote.
fn home(d: &Dirs, file: &str, text: &str) {
    let at = d.j4n_home().join(file);
    fs::create_dir_all(at.parent().unwrap()).unwrap();
    fs::write(at, text).unwrap();
}

#[test]
fn a_fresh_home_loads_exactly_the_basic_packs_and_every_other_pack_waits() {
    let d = Dirs::bare("basic-fresh");
    let w = open(&d);
    let (on, all) = packs(&w);
    assert_eq!(on, ids(&["base", "connect", "crew", "grow", "guard", "lanes"]), "J4n basic: the crew and the lanes are in it, so the word `orchestrate` is the one switch");
    assert_eq!(on, ids(&j4n::pack::ON), "the constant says the same");
    assert_eq!(all, j4n::pack::catalog().ids().into_iter().map(String::from).collect::<BTreeSet<_>>(), "the other packs are built in and listed");
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    // the guard is the redactor's switch: it is in J4n basic, so it is on
    assert!(w.front.screen().packs.iter().any(|p| p.id == "guard" && p.on));
    w.close();
}

#[test]
fn packs_full_loads_every_pack_built_in_and_an_enable_row_still_wins() {
    let d = Dirs::bare("basic-full");
    home(&d, "settings/full.toml", "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n");
    let w = open(&d);
    let (on, all) = packs(&w);
    let mut every = all.clone();
    every.remove("test");
    assert_eq!(on, every, "every pack built in loads, the empty `test` pack apart");
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w.close();
    drop(w);
    // an `enable` row of the operator's is read after the default, both ways
    home(&d, "harness.toml", "api = 1\n[[enable]]\nid = \"qc\"\non = false\n[[enable]]\nid = \"test\"\n");
    let w = open(&d);
    let (on, _) = packs(&w);
    let mut want = every.clone();
    want.remove("qc");
    want.insert("test".into());
    assert_eq!(on, want, "off turns one of J4n full off, on turns the left-out one on");
    w.close();
    drop(w);
    // with the switch off again, it is J4n basic
    home(&d, "settings/full.toml", "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = false\n");
    home(&d, "harness.toml", "api = 1\n");
    let w = open(&d);
    assert_eq!(packs(&w).0, ids(&j4n::pack::ON));
    w.close();
}

/// A home in use: one account, one reflex turned on, the chats on.
fn operator_home(d: &Dirs) {
    home(d, "settings/chats.toml", "api = 1\n[[setting]]\nid = \"chats.on\"\nvalue = true\n");
    home(d, "accounts.toml", "api = 1\n[[account]]\nid = \"claude-2\"\nconnector = \"claude\"\nplan = \"max5\"\n");
    home(d, "reflexes.toml", "api = 1\n[[rule]]\nid = \"compact-junk\"\ndoc = \"Shrink the session when much of it is junk.\"\non = \"task.settled\"\nwhen = \"ctx.grew >= 0.01\"\nthen = \"ask\"\ntext = \"the context holds more than a twentieth of junk\"\ndo = \":compact junk\"\nlimit = { per = \"thread\", max = 1, within = \"10m\" }\noff = false\n");
}

#[test]
fn a_home_that_already_uses_a_pack_keeps_it_and_the_doctor_says_why() {
    let d = Dirs::bare("basic-kept");
    operator_home(&d);
    let w = open(&d);
    let (on, _) = packs(&w);
    // the account is the pack `accounts`' kind, the reflex a rule that the pack `reflexes` ships; `chats.on` is a setting of base
    assert_eq!(on, ids(&["base", "connect", "crew", "grow", "guard", "lanes", "accounts", "reflexes"]), "J4n basic and what this home uses");
    assert!(w.notices.is_empty(), "nothing the home says is lost or refused: {:?}", w.notices);
    let screen = w.front.screen();
    assert_eq!(screen.set.pointer("/chats/on"), j4n_core::value::Value::Bool(true), "chats.on stands");
    assert!(screen.tables.raw("account", "claude-2").is_some(), "the account row is read");
    w.close();
    // the doctor's own lines: the same packs, each with the thing the home has
    let catalog = j4n::pack::catalog();
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &d.j4n_home());
    let (rows, _) = enables(vec![user.clone()]);
    let (_, kept) = j4n::pack::harness(&catalog, &[user], &rows);
    assert_eq!(kept.keys().map(String::as_str).collect::<Vec<_>>(), ["accounts", "reflexes"]);
    assert_eq!(kept["accounts"], "your folder has account claude-2");
    assert_eq!(kept["reflexes"], "your folder has rule compact-junk");
    let said = report(&Facts { kept: kept.into_iter().collect(), ..Facts::default() }).rows;
    let line = |id: &str| said.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no row {id}"));
    assert!(line("pack accounts").kind == "kept" && line("pack accounts").fix.contains("claude-2") && line("pack accounts").fix.contains("Grow › Packs"), "{:?}", line("pack accounts"));
    assert!(!said.iter().any(|r| r.kind == "off" && matches!(r.id.as_str(), "pack accounts" | "pack reflexes")), "a pack that is on is not listed off");
    assert!(said.iter().any(|r| r.kind == "off" && r.id == "pack kit"), "the others are");
    // the operator's own row still turns a kept pack off
    home(&d, "harness.toml", "api = 1\n[[enable]]\nid = \"accounts\"\non = false\n");
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &d.j4n_home());
    let (rows, _) = enables(vec![user.clone()]);
    let (h, _) = j4n::pack::harness(&catalog, &[user], &rows);
    assert_eq!(h.info.iter().filter(|i| i.on).map(|i| i.id.clone()).collect::<BTreeSet<_>>(), ids(&["base", "connect", "crew", "grow", "guard", "lanes", "reflexes"]));
}

#[test]
fn a_pack_a_kept_pack_needs_comes_with_it_and_a_home_that_says_nothing_of_a_pack_keeps_none() {
    let d = Dirs::bare("basic-needs");
    // a setting of the pack `code`, which needs `agent`: both stay, the second for the first
    home(&d, "settings/loop.toml", "api = 1\n[[phrase]]\nid = \"build\"\nwords = [\"/ship\"]\nflow = \"plan-work-validate\"\n");
    let catalog = j4n::pack::catalog();
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &d.j4n_home());
    let (_, kept) = j4n::pack::harness(&catalog, &[user], &[]);
    assert_eq!(kept.get("code").map(String::as_str), Some("your folder has phrase build"));
    assert_eq!(kept.get("agent").map(String::as_str), Some("code needs it"));
    // the settings and rows of base, of the guard and of the other packs' kinds that the home does not use keep nothing
    let d = Dirs::bare("basic-none");
    home(&d, "settings/a.toml", "api = 1\n[[setting]]\nid = \"check.after\"\nvalue = false\n[[setting]]\nid = \"theme.palette\"\nvalue = \"light\"\n");
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &d.j4n_home());
    assert!(j4n::pack::harness(&catalog, &[user], &[]).1.is_empty());
}

#[test]
fn j4n_basic_takes_a_chat_turn_and_a_task_end_to_end_with_no_other_pack() {
    let d = Dirs::bare("basic-turn");
    let w = open(&d);
    assert_eq!(packs(&w).0, ids(&j4n::pack::ON));
    let mut op = Operator::sit(&w);
    // a line is a task of the doer, behind the gate, with its cost line and its Log; a second line is a turn of the same chat.
    // a line typed while the first task still works is a steer of it (the test below), so the second is typed once the first is over
    op.type_line("say hello");
    let first = std::time::Instant::now() + crate::support::PATIENCE;
    while !w.front.tasks().book().rows().first().is_some_and(|r| r.state.is_terminal()) {
        assert!(std::time::Instant::now() < first, "the first turn did not end");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    op.type_line("say hello again");
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while w.front.tasks().book().rows().len() < 2 || !w.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()) {
        assert!(std::time::Instant::now() < end, "the turns did not end: {:?}", w.front.tasks().book().rows().iter().map(|r| &r.state).collect::<Vec<_>>());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let rows = w.front.tasks().book().rows();
    assert!(rows.iter().all(|r| matches!(r.state, j4n_core::task::State::Done { ok: true })), "{:?}", rows.iter().map(|r| &r.state).collect::<Vec<_>>());
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w.close();
}

/// what the second line of the test above is when the first task is still going (a machine under load, a doer that takes its time): the line is
/// not lost and not a second task, it is a steer of the task that works, written to the Log as the operator's own words and handed to the doer.
#[test]
fn a_line_typed_while_a_task_works_steers_it_and_is_neither_lost_nor_a_second_task() {
    let d = Dirs::bare("j4n-basic-steer");
    let w = open(&d);
    let mut op = Operator::sit(&w);
    op.type_line("hang");
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    let running = || w.front.tasks().book().rows().first().is_some_and(|r| matches!(r.state, j4n_core::task::State::Running));
    while !running() {
        assert!(std::time::Instant::now() < end, "the first task never ran: {:?}", w.front.tasks().book().rows().iter().map(|r| &r.state).collect::<Vec<_>>());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    op.type_line("say hello again");
    let steered = |d: &Dirs| crate::run_cases::log_events(d).iter().filter(|e| e["kind"] == "task.steered" && e["data"]["text"] == "say hello again").count();
    while steered(&d) == 0 {
        assert!(std::time::Instant::now() < end, "the line was not steered into the task: {:?}", crate::run_cases::kinds(&crate::run_cases::log_events(&d)));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(w.front.tasks().book().rows().len(), 1, "one task: the line is a steer of it, not a second task");
    assert_eq!(steered(&d), 1, "once");
    let id = w.front.tasks().book().rows()[0].id.clone();
    let _ = w.front.control("main", &id, j4n_proto::Control::Abort);
    w.close();
}

/// What the doctor says of a home, through the same functions `j4n doctor` and the Setup page call.
fn doctor_rows(d: &Dirs) -> Vec<j4n_core::doctor::Row> {
    let catalog = j4n::pack::catalog();
    let (user, _) = Layer::from_dir(LayerKind::User, "home", &d.j4n_home());
    let (rows, _) = enables(vec![user.clone()]);
    let (harness, kept) = j4n::pack::harness(&catalog, std::slice::from_ref(&user), &rows);
    let full = j4n_core::pack::says_full(std::slice::from_ref(&user));
    report(&Facts { kept: j4n::pack::why_on(&harness, &kept, &rows, full), full, ..Facts::default() }).rows
}

#[test]
fn a_pack_an_enable_row_turns_on_is_on_in_the_doctor_with_the_reason_and_not_off() {
    let d = Dirs::bare("j4n-basic-enable");
    home(&d, "harness.toml", "api = 1\n[[enable]]\nid = \"habits\"\n");
    let said = doctor_rows(&d);
    let line = |id: &str| said.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("no row {id}"));
    let habits = line("pack habits");
    assert!(habits.kind == "kept" && habits.what == "on" && habits.fix.contains("enable row"), "{habits:?}");
    assert!(!said.iter().any(|r| r.kind == "off" && r.id == "pack habits"), "an enabled pack is not listed off");
    // the others are as they were, and the switch of J4n full is off
    assert!(said.iter().any(|r| r.kind == "off" && r.id == "pack kit") && said.iter().any(|r| r.kind == "off" && r.id == "packs.full"));
    // a pack that is turned on by a row and also used by the folder says what the folder has
    home(&d, "reflexes.toml", "api = 1\n[[rule]]\nid = \"compact-junk\"\ndoc = \"x\"\non = \"task.settled\"\nwhen = \"ctx.grew >= 0.01\"\nthen = \"ask\"\ntext = \"t\"\ndo = \":compact junk\"\nlimit = { per = \"thread\", max = 1, within = \"10m\" }\noff = false\n");
    home(&d, "harness.toml", "api = 1\n[[enable]]\nid = \"reflexes\"\n");
    let said = doctor_rows(&d);
    assert!(said.iter().any(|r| r.id == "pack reflexes" && r.kind == "kept" && r.fix.contains("your folder has rule compact-junk")), "{said:?}");
}

#[test]
fn under_packs_full_every_pack_is_on_with_that_reason_and_the_setting_is_not_listed_off() {
    let d = Dirs::bare("j4n-basic-full");
    home(&d, "settings/full.toml", "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n");
    let said = doctor_rows(&d);
    let off: Vec<&str> = said.iter().filter(|r| r.kind == "off" && r.id.starts_with("pack ")).map(|r| r.id.as_str()).collect();
    assert_eq!(off, ["pack test"], "the empty `test` pack is the one pack J4n full leaves out: {off:?}");
    let on: Vec<&str> = said.iter().filter(|r| r.kind == "kept" && r.id.starts_with("pack ")).map(|r| r.id.as_str()).collect();
    assert!(on.contains(&"pack habits") && on.contains(&"pack ledger") && on.contains(&"pack shape") && on.contains(&"pack qc"), "{on:?}");
    assert!(said.iter().filter(|r| r.kind == "kept" && r.id.starts_with("pack ")).all(|r| r.what == "on" && r.fix.contains("packs.full is on")), "{said:?}");
    assert!(!said.iter().any(|r| r.id == "packs.full" && r.kind == "off"), "the switch is on and is not listed off");
    let own = said.iter().find(|r| r.id == "packs.full").expect("a row of its own");
    assert!(own.kind == "kept" && own.what == "on", "{own:?}");
    // an enable row that turns one off wins over J4n full, and the doctor lists it off again
    home(&d, "harness.toml", "api = 1\n[[enable]]\nid = \"habits\"\non = false\n");
    let said = doctor_rows(&d);
    assert!(!said.iter().any(|r| r.kind == "kept" && r.id == "pack habits"), "{said:?}");
}

#[test]
fn a_basic_home_says_what_it_did_before() {
    let d = Dirs::bare("j4n-basic-home");
    let said = doctor_rows(&d);
    assert!(!said.iter().any(|r| r.kind == "kept"), "nothing is kept in a home that says nothing");
    assert_eq!(said.iter().filter(|r| r.kind == "off" && r.id.starts_with("pack ")).count(), 13 + 5, "the thirteen packs basic leaves off and the five of their own reason");
    assert!(said.iter().any(|r| r.kind == "off" && r.id == "packs.full"));
}

#[test]
fn chats_on_in_a_pipe_says_the_chats_are_on_and_only_a_screen_shows_them_and_off_still_says_off() {
    let said = |d: &Dirs, screen: bool| {
        let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
        let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
        let w = crate::support::opened(|| wire::open(setup.clone(), &opts, screen, "main"));
        let got = w.front.say("main", ":chats");
        w.close();
        match got {
            j4n_core::front::Said::Refused(why) => why,
            other => panic!("`:chats` was not refused: {other:?}"),
        }
    };
    let d = Dirs::bare("j4n-basic-on");
    home(&d, "settings/chats.toml", "api = 1\n[[setting]]\nid = \"chats.on\"\nvalue = true\n");
    let pipe = said(&d, false);
    assert!(pipe.contains("the chats are on, but only a screen shows them") && !pipe.contains("are off"), "{pipe}");
    // the switch off: the old line, which leads to the switch
    let d = Dirs::bare("j4n-basic-off");
    assert!(said(&d, false).contains("the chats are off") && said(&d, false).contains("chats.on"));
}

/// `brief.cli` is `claude` in a home that has chosen nothing, so claude (and no other doer) is given the contract and the project card after its goal; a row of the operator's turns it off.
#[test]
fn brief_cli_is_on_by_default_and_the_operators_row_turns_it_off() {
    let d = Dirs::bare("j4n-basic-default");
    let w = open(&d);
    assert_eq!(w.front.screen().set.pointer("/brief/cli"), j4n_core::value::Value::text("claude"));
    w.close();
    let d = Dirs::bare("j4n-basic-default-off");
    home(&d, "settings/mine.toml", "api = 1\n[[setting]]\nid = \"brief.cli\"\nvalue = \"off\"\n");
    let w = open(&d);
    assert_eq!(w.front.screen().set.pointer("/brief/cli"), j4n_core::value::Value::text("off"), "the operator's row wins");
    w.close();
}

/// the doctor of a home under `packs.full` says each switch that is on because of it, and the ones it leaves to the operator.
#[test]
fn the_doctor_lists_what_packs_full_turned_on_and_what_it_leaves() {
    let d = Dirs::bare("j4n-basic-doctor");
    home(&d, "settings/full.toml", "api = 1\n[[setting]]\nid = \"packs.full\"\nvalue = true\n");
    let out = crate::run_cases::j4n(&d, &["doctor"], None, &[]);
    let text = format!("{}{}", out.out, out.err);
    for id in ["kit.mode", "lanes", "qc.on", "shape.on", "reflexes.on", "offload.on", "parts.mode"] {
        assert!(text.lines().any(|l| l.contains(id) && l.contains("on because of packs.full")), "{id}:\n{text}");
    }
    assert!(text.contains("packs.full leaves") && text.contains("contribute.mode") && text.contains("each sends text or a line out of the project"), "{text}");
    // a row of the operator's takes one back: the doctor does not claim it
    home(&d, "settings/mine.toml", "api = 1\n[[setting]]\nid = \"qc.on\"\nvalue = false\n");
    let out = crate::run_cases::j4n(&d, &["doctor"], None, &[]);
    let text = format!("{}{}", out.out, out.err);
    assert!(!text.lines().any(|l| l.contains("qc.on") && l.contains("on because of packs.full")) && text.lines().any(|l| l.contains("shape.on") && l.contains("on because of packs.full")), "{text}");
    // a basic home says none of it
    let b = Dirs::bare("j4n-basic-doctor-basic");
    let text = { let o = crate::run_cases::j4n(&b, &["doctor"], None, &[]); format!("{}{}", o.out, o.err) };
    assert!(!text.contains("on because of packs.full") && !text.contains("packs.full leaves"), "{text}");
}

fn ids_of(rows: &[j4n_core::value::Value]) -> BTreeSet<String> {
    rows.iter().filter_map(|r| match r {
        j4n_core::value::Value::Map(m) => match m.get("id") {
            Some(j4n_core::value::Value::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }).collect()
}

/// In J4n basic no pack names a model and no connector speaks an openai or anthropic API. The library still reads the connectors' own
/// models, and the base pack names the providers, so a bare home has both.
#[test]
fn in_j4n_basic_the_library_has_claudes_models_and_the_providers_have_anthropic_and_openrouter() {
    let d = Dirs::bare("basic-library");
    let w = open(&d);
    assert_eq!(packs(&w).0, ids(&j4n::pack::ON));
    let models = ids_of(&w.front.library_rows());
    assert!(["haiku", "sonnet", "opus"].iter().all(|m| models.contains(*m)), "claude's models are in the library: {models:?}");
    let providers = ids_of(&w.front.provider_rows());
    let want = ids(&["anthropic", "openrouter", "openai", "google", "local", "pi", "omp", "opencode"]);
    assert!(want.is_subset(&providers), "the providers: {providers:?}");
    w.close();
}

/// What `single` writes to the Log for two lines, as the count of each kind of event.
fn single_kinds(d: &Dirs) -> std::collections::BTreeMap<String, usize> {
    let w = open(d);
    assert_eq!(w.front.lanes_now(), j4n_core::branch::Switch::Single);
    let mut op = Operator::sit(&w);
    for (n, line) in ["say hello", "say hello again"].into_iter().enumerate() {
        op.type_line(line);
        // the line is its own task (the count reaches the line's number), and every task is over
        let end = std::time::Instant::now() + crate::support::PATIENCE;
        while w.front.tasks().book().rows().len() <= n || !w.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()) {
            assert!(std::time::Instant::now() < end, "the turn did not end");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // and the Log has stopped growing: what a turn writes after it ends (a fold, a card) is counted too
        let mut last = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let now = w.front.log().read(0).count();
            if now == last {
                break;
            }
            last = now;
        }
    }
    let mut kinds = std::collections::BTreeMap::new();
    for e in w.front.log().read(0) {
        *kinds.entry(e.kind).or_insert(0) += 1;
    }
    drop(op);
    w.close();
    kinds
}

/// The crew and the lanes are loaded in J4n basic and act only under the word `orchestrate`: under `single` the Log of the same two lines holds the same kinds
/// as with both packs turned off (the lines carry no ids, so the counts are the whole of it).
#[test]
fn under_single_the_crew_and_the_lanes_packs_in_basic_change_nothing_in_the_log() {
    let with = Dirs::bare("j4n-basic-with");
    let without = Dirs::bare("j4n-basic-without");
    home(&without, "harness.toml", "api = 1\n[[enable]]\nid = \"crew\"\non = false\n[[enable]]\nid = \"lanes\"\non = false\n");
    let w = open(&with);
    assert!(packs(&w).0.is_superset(&ids(&["crew", "lanes"])), "{:?}", packs(&w).0);
    assert!(w.front.cfg().branches.allows(j4n_core::branch::Class::Scout, j4n_core::branch::Switch::Auto), "the lanes' classes are loaded");
    assert!(!w.front.cfg().crew.on && !w.front.cfg().orchestrate.look && !w.front.cfg().crew.envoys, "and under single the switches that follow the word are off");
    w.close();
    drop(w);
    let w = open(&without);
    assert!(!packs(&w).0.contains("crew") && !packs(&w).0.contains("lanes"));
    w.close();
    drop(w);
    assert_eq!(single_kinds(&with), single_kinds(&without));
}

/// Under the word `orchestrate` the doctor says the word and does not list the switch off; under `single` it lists the switch off and says what it turns on.
#[test]
fn the_doctor_under_the_word_orchestrate_says_the_word_and_does_not_list_the_switch_off() {
    let text = |d: &Dirs| {
        let o = crate::run_cases::j4n(d, &["doctor"], None, &[]);
        format!("{}{}", o.out, o.err)
    };
    let single = Dirs::bare("j4n-basic-doctor-single");
    let said = text(&single);
    assert!(said.lines().any(|l| l.starts_with("off") && l.contains("lanes") && l.contains("`orchestrate` is the one switch")), "the one switch is listed off under single:\n{said}");
    let d = Dirs::bare("j4n-basic-doctor-word");
    home(&d, "settings/word.toml", "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n");
    let said = text(&d);
    assert!(!said.lines().any(|l| l.starts_with("off") && l.contains("lanes")), "the switch is not listed off under the word:\n{said}");
    assert!(said.lines().any(|l| l.contains("lanes") && l.contains("orchestrate") && l.contains("follow the word")), "{said}");
}
