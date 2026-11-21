//! A file of the operator's changes the template `main` and the screen is laid out as it says, with no change to
//! the core or the TUI; a pack's page under `home` adds a tab. The front and the TUI are the real ones, wired as the binary wires them.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;
use std::fs;

fn put(d: &Dirs, rel: &str, text: &str) {
    let p = d.j4n_home().join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

const BAR_FIRST: &str = "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ id = \"bar\", field = \"bar\", bind = \"bar:status\" }, { id = \"pages\", tabs = \"home\", grow = 1 }]\n";

const SIDE: &str = r#"api = 1
[[page]]
id = "side"
parent = "home"
title = "Side"
order = 500
present = "split"
ratio = 50
children = [{ field = "list", bind = "tasks:all" }, { field = "log", bind = "log:all" }]
[[command]]
id = "side"
open = "side"
"#;

fn lines(s: &str) -> Vec<&str> {
    s.lines().collect()
}

#[test]
fn a_file_in_the_operators_folder_with_the_id_main_lays_out_the_screen_with_no_code_change() {
    // as shipped: the tabs on top, and the bottom row: the keys while the screen is idle, the status bar's place the same row
    let d = Dirs::new("tpl-shipped");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    // the keys come from the core on a message of their own, after the tabs: both are waited for
    let s = op.until("the tabs and the keys row", |s| s.lines().nth(1).is_some_and(|l| l.contains("Chat")) && lines(s).last().is_some_and(|l| l.contains("⌥1–9 tabs")));
    assert!(lines(&s).last().is_some_and(|l| l.contains("⌥1–9 tabs") && l.contains("? keys") && !l.contains("idle")), "the last row is the keys while idle, with no status: {s}");
    assert!(lines(&s).iter().rev().nth(1).is_some_and(|l| l.starts_with('╰')), "and the composer is the row over it, with no bar between: {s}");
    world.close();
    // the operator's file puts the bar over the tabs and leaves the questions out
    let d = Dirs::new("tpl-bar-first");
    put(&d, "ui/templates.toml", BAR_FIRST);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    let s = op.until("the bar over the tabs", |s| lines(s).first().is_some_and(|l| l.contains("idle")) && s.contains("Chat"));
    let ls = lines(&s);
    assert!(ls[0].contains("idle") && ls[2].contains("Chat"), "the bar is the first row and the tabs under a row of room: {s}");
    assert!(ls.last().is_some_and(|l| l.starts_with('╰')), "and the composer is the last: {s}");
    world.close();
}

#[test]
fn a_file_with_a_mistake_is_said_and_the_screen_has_what_the_pack_gave() {
    let d = Dirs::new("tpl-mistake");
    put(&d, "ui/templates.toml", "api = 1\n[[template]]\nid = \"main\"\nchildren = [{ field = \"bar\", bind = \"bar:status\" }, { use = \"gone\" }]\n");
    let world = open(&d);
    let said = world.notices.join("\n");
    assert!(said.contains("[[template]] main part 2") && said.contains("there is none"), "{said:?}");
    world.close();
}

#[test]
fn a_page_of_the_operators_with_parts_of_its_own_is_a_tab_and_shows_them_side_by_side() {
    let d = Dirs::new("tpl-side");
    put(&d, "ui/pages.toml", SIDE);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    let s = op.until("the new tab", |s| s.lines().nth(1).is_some_and(|l| l.contains("Side")));
    assert!(s.lines().nth(1).unwrap().contains("Chat"), "{s}");
    op.type_line("say it is done");
    op.until("the answer", |s| s.contains("it is done"));
    op.type_line(":side");
    // the two panes are sent apart: the task is waited for left of the bar, not only the log's own row that says the same words
    let s = op.until("the tasks beside the log", |s| s.contains("task.settled") && s.lines().any(|l| l.find("say it is done").zip(l.rfind('┃')).is_some_and(|(t, bar)| t < bar)));
    // the task is the row that has the words left of the bar (the log may say them on a row of its own, above or below)
    let task_at = s.lines().find_map(|l| l.find("say it is done").zip(l.rfind('┃')).filter(|(t, bar)| t < bar));
    assert!(task_at.is_some(), "the tasks are the first pane, left of the bar between the panes: {s}");
    let (log, bar) = s.lines().find(|l| l.contains("task.settled")).and_then(|l| l.find("task.settled").zip(l.rfind('┃'))).expect("the log");
    assert!(bar < log, "and the log the second: {s}");
    world.close();
}
