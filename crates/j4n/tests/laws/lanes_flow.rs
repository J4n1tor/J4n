//! `:lanes` says the one switch while J4n runs. The word is written for this project as a
//! proposal that is accepted at once and that `:undo` takes back; the strip shows it, the pack's branch rows load clean, and the next
//! start reads the word from the file. The front, the proposer, the Writer and the screen are the real ones, wired as the binary wires them.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::branch::{Class, Switch};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use std::fs;

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn held(d: &Dirs) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join("lanes.toml")).unwrap_or_default()
}

#[test]
fn the_word_is_single_until_said_is_drawn_on_the_strip_is_written_for_the_project_and_undo_takes_it_back() {
    let d = Dirs::new("lanes-live");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    assert_eq!(world.front.lanes_now(), Switch::Single, "single is the default");
    let rows = world.front.cfg().branches.clone();
    assert!(rows.allows(Class::Scout, Switch::Auto) && !rows.allows(Class::Piece, Switch::Auto) && rows.allows(Class::Piece, Switch::Wide), "the pack's rows are loaded");
    let mut op = Operator::sit(&world);
    op.type_line(":lanes");
    op.until("the words said", |s| s.contains("[single] · auto · wide"));
    op.type_line("hello");
    // the strip names the switch only when it is not `single`
    let s = op.until("the line is on the screen", |s| s.contains("hello"));
    assert!(!s.contains("lanes single"), "{s}");
    op.type_line(":lanes auto");
    let said = op.until("said back", |s| s.contains("lanes is auto"));
    assert!(!said.contains("the pack lanes is off"), "the pack is on here, and nothing is said of it: {said}");
    assert_eq!(world.front.lanes_now(), Switch::Auto);
    let file = held(&d);
    assert!(file.contains("id = \"lanes\"") && file.contains("value = \"auto\""), "{file}");
    op.until("the strip follows", |s| s.contains("lanes auto"));
    // the next start reads it from the file
    drop(op);
    world.close();
    drop(world);
    let again = open(&d);
    assert_eq!(again.front.cfg().lanes, Switch::Auto);
    let mut op = Operator::sit(&again);
    op.type_line(":undo");
    op.until("the word taken back", |s| s.contains("took back 1 change"));
    assert_eq!(again.front.lanes_now(), Switch::Single);
    assert!(!held(&d).contains("value = \"auto\""), "{}", held(&d));
    drop(op);
    again.close();
}

#[test]
fn a_word_that_is_not_one_is_refused_the_same_word_is_said_already_and_nothing_is_written() {
    let d = Dirs::new("lanes-refused");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":lanes narrow");
    op.until("the words said", |s| s.contains("narrow is not a word of lanes") && s.contains("single, auto, wide"));
    op.type_line(":lanes single");
    op.until("said already", |s| s.contains("lanes is single already"));
    op.type_line(":lanes wide");
    op.until("wide is said", |s| s.contains("lanes is wide"));
    assert!(held(&d).contains("value = \"wide\""), "{}", held(&d));
    assert_eq!(world.front.lanes_now(), Switch::Wide);
    drop(op);
    world.close();
}

#[test]
fn the_front_knows_the_doer_check_so_a_held_pieces_checks_can_start() {
    // the front knows the doer `check`, so `validate` can start a held piece's checks in the binary
    let d = Dirs::new("lanes-check-known");
    let world = open(&d);
    assert!(world.front.knows_doer("check"));
    world.close();
}

#[test]
fn the_tier_settings_are_empty_until_set_and_a_file_in_the_projects_folder_sets_the_planner_and_the_reviewer() {
    let d = Dirs::new("tiers");
    let world = open(&d);
    let cfg = world.front.cfg();
    assert!(cfg.planner.doer.is_none() && cfg.planner.model.is_none() && cfg.reviewer.doer.is_none() && cfg.reviewer.model.is_none(), "the thread's own until set");
    assert!(cfg.cheap.doer.is_none() && cfg.cheap.model.is_none(), "and the cheap branches' too");
    drop(cfg);
    world.close();
    drop(world);
    let dir = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("tiers.toml"), "api = 1\n\n[[setting]]\nid = \"plan.model\"\nvalue = \"opus\"\n\n[[setting]]\nid = \"plan.review_doer\"\nvalue = \"claude\"\n\n[[setting]]\nid = \"plan.cheap_model\"\nvalue = \"haiku\"\n").unwrap();
    let again = open(&d);
    let cfg = again.front.cfg();
    assert_eq!((cfg.planner.doer.as_deref(), cfg.planner.model.as_deref()), (None, Some("opus")));
    assert_eq!((cfg.reviewer.doer.as_deref(), cfg.reviewer.model.as_deref()), (Some("claude"), None));
    assert_eq!((cfg.cheap.doer.as_deref(), cfg.cheap.model.as_deref()), (None, Some("haiku")));
    assert!(again.notices.is_empty(), "{:?}", again.notices);
}

#[test]
fn the_word_orchestrate_turns_the_chats_on_and_single_turns_them_off_and_leaves_no_trace_of_the_mode() {
    // while the word is `orchestrate` the chats act as on, whatever `chats.on` says; `:lanes single` ends it at once, and the round
    // trip leaves no session, no chat and no Log kind of the mode
    let d = Dirs::new("lanes-orchestrate");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    assert!(!world.front.chats_on(), "single: the chats are off");
    let mut op = Operator::sit(&world);
    op.type_line(":lanes");
    op.until("the four words said", |s| s.contains("[single] · auto · wide · orchestrate"));
    op.type_line(":lanes orchestrate");
    op.until("said back", |s| s.contains("lanes is orchestrate"));
    assert_eq!(world.front.lanes_now(), Switch::Orchestrate);
    assert!(world.front.chats_on() && world.front.main_works(), "the word turns the chats on");
    assert!(held(&d).contains("value = \"orchestrate\""), "{}", held(&d));
    op.type_line(":lanes single");
    op.until("said back", |s| s.contains("lanes is single"));
    assert_eq!(world.front.lanes_now(), Switch::Single);
    assert!(!world.front.chats_on() && !world.front.main_works(), "single turns them off again");
    let kinds: Vec<String> = world.front.log().read(0).map(|e| e.kind).collect();
    for gone in ["thread.bound", "chat.opened", "chat.routed", "chat.route.asked", "chat.message", "task.queued"] {
        assert!(!kinds.iter().any(|k| k == gone), "{gone} in {kinds:?}");
    }
    assert!(!kinds.iter().any(|k| k.contains("orchestrate")), "no Log kind of the mode: {kinds:?}");
    assert!(world.front.chats().is_empty() && world.front.binding("main").is_none(), "no chat, no session");
    drop(op);
    world.close();
}

#[test]
fn said_with_the_pack_off_the_switch_tells_that_no_scout_reviewer_or_piece_will_start() {
    // with the pack `lanes` off, `lanes = auto` keeps the plan and starts no branch, and `:lanes auto` says so, not only "lanes is auto"
    let d = Dirs::bare("lanes-pack-off");
    fs::create_dir_all(d.j4n_home()).unwrap();
    fs::write(d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"lanes\"\non = false\n").unwrap();
    let world = open(&d);
    assert!(!world.front.cfg().branches.allows(Class::Scout, Switch::Auto), "with the pack turned off there are no branch rows");
    let mut op = Operator::sit(&world);
    op.type_line(":lanes auto");
    op.until("the pack is said to be off", |s| s.contains("lanes is auto") && s.contains("the pack lanes is off"));
    drop(op);
    world.close();
}

#[test]
fn turning_the_word_orchestrate_on_or_off_marks_mains_thread_fresh_and_the_next_line_starts_its_own_session() {
    // a line typed before the switch binds a session of Main's; the switch must not carry it into the mode
    let d = Dirs::bare("lanes-fresh");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line("say hello");
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while world.front.binding("main").is_none() || !world.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()) {
        assert!(std::time::Instant::now() < end, "the first turn did not end");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let fresh = |w: &World| w.front.log().read(0).filter(|e| e.kind == "thread.fresh").count();
    assert_eq!(fresh(&world), 0);
    op.type_line(":lanes orchestrate");
    op.until("said back", |s| s.contains("lanes is orchestrate"));
    assert_eq!(fresh(&world), 1, "the switch marked Main's thread fresh");
    assert!(world.front.binding("main").is_none(), "no session is bound: the next line starts Main's own");
    // the next line starts Main's own session: no `follows` the session bound before the switch
    op.type_line("say hello again");
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while world.front.tasks().book().rows().len() < 2 || !world.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()) {
        assert!(std::time::Instant::now() < end, "the second turn did not end");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let rows = world.front.tasks().book().rows();
    assert!(rows[0].spec.follows.is_none(), "the line before the switch was a first turn");
    assert!(rows[1].spec.follows.is_none(), "the line after it does not follow the session bound before the switch: {:?}", rows[1].spec.follows);
    // a switch to the same word, and an edit of another setting, do not
    op.type_line(":lanes orchestrate");
    op.until("refused", |s| s.contains("lanes is orchestrate already"));
    assert_eq!(fresh(&world), 1);
    // and the other way
    op.type_line(":lanes single");
    op.until("said back", |s| s.contains("lanes is single"));
    assert_eq!(fresh(&world), 2, "single is a new start too");
    // `:lanes auto` is neither: the word `orchestrate` did not change
    op.type_line(":lanes auto");
    op.until("said back", |s| s.contains("lanes is auto"));
    assert_eq!(fresh(&world), 2);
    // `:undo` of the word is a switch too
    let wait = |what: &str, done: &dyn Fn() -> bool| {
        let end = std::time::Instant::now() + crate::support::PATIENCE;
        while !done() {
            assert!(std::time::Instant::now() < end, "{what}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    op.type_line(":lanes orchestrate");
    wait("the word was not set again", &|| fresh(&world) == 3);
    op.type_line(":undo");
    wait("the undo of the word was not a new start", &|| fresh(&world) == 4);
    assert_eq!(world.front.lanes_now(), Switch::Auto, "back to what it was");
    drop(op);
    world.close();
}
