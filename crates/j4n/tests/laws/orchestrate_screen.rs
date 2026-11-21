//! What the operator sees of the word `orchestrate`, on the real screen with a `claude` that answers by what it is asked: the word on the strip; one head for the quick
//! look (who read the line, what it was taken as, at what cost, and why) and none for its JSON; the chat Main started in the list while it works, with its brief as the
//! head of its first row; Main's row of its end in the chat's own words, and the digest; and the catalogue Main reads, on a page of its own.

use crate::orchestrate_run::{claude_slow_look, settings};
use crate::run_cases::Dirs;
use crate::screen_drive::Operator;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup};
use j4n_core::task::Mode;
use j4n_tui::headless::{KeyCode, ctrl, press as key, typed};
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn open(tag: &str, pause: u32) -> (Dirs, j4n::wire::World) {
    let d = Dirs::bare(tag);
    let program = d.dir.join("bin/claude");
    fs::write(&program, claude_slow_look(pause, 2)).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    settings(&d, "orchestrate");
    let opts = Opts { doer: Some("claude".into()), model: Some("haiku".into()), approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: program.display().to_string() };
    let world = crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"));
    (d, world)
}

#[test]
fn the_screen_shows_the_look_as_one_row_the_chat_while_it_works_its_brief_and_its_end() {
    let (_d, world) = open("om-screen", 2);
    let mut op = Operator::sit(&world);
    op.type_line("explain what the code in src/a does");
    // the line is on the screen while the look is still being made (the look takes two seconds here)
    let s = op.until("the line", |s| s.contains("explain what the code in src/a does"));
    assert!(!s.contains("read as"), "{s}");
    // the look is one row, with the model, the cost and the reason, and the JSON it answered is on no row
    // the strip is drawn from a message of its own, which a loaded machine sends after the look's row
    let s = op.until("the look's row and the word on the strip", |s| s.contains("read · ") && s.contains("split · 1 chat") && s.contains("lanes orchestrate"));
    assert!(s.contains("lanes orchestrate"), "the word is on the strip: {s}");
    // the read is a head of its own in the group view: who read it, what it was taken as and what it cost, and why under it
    let row = s.lines().find(|l| l.contains("read · ")).unwrap();
    assert!(row.contains("haiku 5.5") && row.contains('$') && row.contains("split · 1 chat"), "{row}");
    assert!(s.contains("one part apart") && !s.contains("read as split"), "{s}");
    assert!(!s.contains("\"decision\"") && !s.contains("\"chats\""), "no row shows what the look or the planning turn answered: {s}");
    // while the chat works it is in the list, and the plan line counts it
    let _ = op.until("the chat at work", |s| s.contains("c1 read the code") && s.contains("split · c1"));
    let _ = op.until("the plan line", |s| s.contains("plan 0/1"));
    op.up.send(j4n_proto::Up::Intent { intent: j4n_proto::Intent::Enter { chat: "c1".into() } }).unwrap();
    let s = op.until("the chat", |s| s.contains("project › c1") && s.contains("brief · reads"));
    let head = s.lines().find(|l| l.starts_with('▌')).unwrap_or_default();
    assert!(head.contains("read the code and say what it does"), "the brief heads the first row: {s}");
    assert!(s.contains("brief · reads · claude"), "{s}");
    op.up.send(j4n_proto::Up::Intent { intent: j4n_proto::Intent::Leave }).unwrap();
    // its end: Main's row has the chat's own words, and the digest says the rest
    let s = op.until("the end", |s| s.contains("scout reviewed") && s.contains("it parses lines"));
    assert!(!s.contains("\"found\""), "no row shows the report as JSON in Main: {s}");
    drop(op);
    world.close();
}

#[test]
fn the_catalogue_is_a_page_of_its_own_that_says_what_main_is_told_and_leads_to_the_switch_that_reads_it() {
    let (d, world) = open("om-catalogue", 0);
    // the quick look off: the page says so, and its key turns it on at once, and `u` takes that back
    let file = d.home().join(".j4n/settings/orchestrate.toml");
    drop(world);
    let text = fs::read_to_string(&file).unwrap().replace("id = \"orchestrate.look\"\nvalue = true", "id = \"orchestrate.look\"\nvalue = false");
    fs::write(&file, text).unwrap();
    let opts = Opts { doer: Some("claude".into()), model: Some("haiku".into()), approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: d.dir.join("bin/claude").display().to_string() };
    let world = crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"));
    let mut op = Operator::sit(&world);
    op.press(vec![ctrl('k')]);
    op.press(typed("Catalogue", false));
    op.press(vec![key(KeyCode::Enter)]);
    let s = op.until("the page", |s| s.contains("quick look") && s.contains("Catalogue"));
    assert!(s.contains("off · ⏎ turns it on"), "{s}");
    // what Main is told is what the page lists: every feature the text names is a row, and the doer
    let told = world.front.catalogue();
    for id in &told.packs {
        assert!(s.contains(&format!("{id} ")), "{id} is told to Main and is not on the page: {s}");
    }
    assert!(s.contains("claude") && s.contains("doer"), "{s}");
    op.press(vec![key(KeyCode::Enter)]);
    let s = op.until("turned on", |s| s.contains("a line under orchestrate is read against this list first"));
    assert!(world.front.cfg().orchestrate.look, "the key acts at once: {s}");
    op.press(typed("u", false));
    op.until("taken back", |s| s.contains("off · ⏎ turns it on"));
    assert!(!world.front.cfg().orchestrate.look);
    drop(op);
    world.close();
}
