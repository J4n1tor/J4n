//! A file of the operator's rebinds the screen's keys, frees one, and gives a page a key of its own, with no change to
//! the core or the TUI; the chords tmux takes are refused at start with their reason and the rest of the file stands. The front and the
//! TUI are the real ones, wired as the binary wires them.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::task::Mode;
use j4n_tui::headless::{KeyCode, alt, ctrl, press as key, typed};
use std::fs;

fn put(d: &Dirs, rel: &str, text: &str) {
    let p = d.j4n_home().join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

pub fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

const MINE: &str = r#"api = 1
[[key]]
id = "global/palette"
keys = ["C-k", "C-p"]
[[key]]
id = "composer/clear"
keys = []
[[key]]
id = "composer/older"
keys = ["C-o"]
"#;

const SIDE: &str = r#"api = 1
[[page]]
id = "side"
parent = "home"
title = "Side"
order = 500
present = "split"
children = [{ field = "list", bind = "tasks:all" }, { field = "log", bind = "log:all" }]
[[command]]
id = "side"
open = "side"
[[key]]
id = "page:side/home"
keys = ["x"]
run = { intent = "open", node = "chat" }
"#;

#[test]
fn as_shipped_every_key_is_the_screens_own_and_the_map_is_there() {
    let d = Dirs::new("keys-shipped");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    assert!(op.app.keymap.is_some(), "the core said what the keys do");
    op.press(vec![ctrl('k')]);
    assert!(op.app.jump.is_some(), "⌃k finds anything");
    op.press(vec![key(KeyCode::Esc), alt('2')]);
    op.until("the second tab", |s| s.lines().nth(1).is_some_and(|l| l.contains("Tasks")));
    assert!(op.app.jump.is_none() && op.app.tab == 1, "⌥2 is the second tab");
    op.press(vec![alt('1')]);
    op.press(typed("abc", false));
    op.press(vec![ctrl('u')]);
    assert!(op.app.line.buf.is_empty(), "⌃u clears the line");
    world.close();
}

#[test]
fn the_nine_number_keys_of_a_question_are_one_row_of_the_keys_page() {
    // `ask/pick1` … `ask/pick9` were nine rows
    let text = crate::support::read_text(&crate::support::root().join("packs/base/keys/main.toml")).unwrap();
    let picks: Vec<&str> = text.lines().filter(|l| l.starts_with("id = \"ask/pick")).collect();
    assert_eq!(picks, ["id = \"ask/pick\""]);
    assert!(text.contains("1–9") && text.contains("native = [\"1\", \"2\", \"3\", \"4\", \"5\", \"6\", \"7\", \"8\", \"9\"]"), "the one row holds the nine keys, and says 1–9");
}

#[test]
fn a_file_in_the_operators_folder_adds_a_key_moves_one_and_frees_one_with_no_code_change() {
    let d = Dirs::new("keys-mine");
    put(&d, "ui/keys.toml", MINE);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.press(vec![ctrl('p')]);
    assert!(op.app.jump.is_some(), "⌃p, which the file adds, finds anything");
    op.press(vec![key(KeyCode::Esc)]);
    op.press(typed("abc", false));
    op.press(vec![ctrl('u')]);
    assert_eq!(op.app.line.buf, "abc", "⌃u is freed");
    op.press(vec![ctrl('c')]);
    assert!(op.app.quit, "and a key the file does not name is what it was");
    world.close();
    // ⌃o is the history now and ↑ is not
    let d = Dirs::new("keys-mine-history");
    put(&d, "ui/keys.toml", MINE);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.press(typed("first line", true));
    op.press(vec![key(KeyCode::Up)]);
    assert!(op.app.line.buf.is_empty(), "↑ no longer recalls the line");
    op.press(vec![ctrl('o')]);
    assert_eq!(op.app.line.buf, "first line", "⌃o does");
    world.close();
}

#[test]
fn a_chord_tmux_takes_is_refused_at_start_with_its_reason_and_the_rest_of_the_file_stands() {
    let d = Dirs::new("keys-refused");
    put(&d, "ui/keys.toml", "api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"C-b\", \"M-Enter\", \"C-p\"]\n");
    let world = open(&d);
    let said = world.notices.join("\n");
    assert!(said.contains("[[key]] global/palette") && said.contains("`C-b` is not bound: tmux takes it"), "{said:?}");
    assert!(said.contains("`M-Enter`"), "{said:?}");
    let mut op = Operator::sit(&world);
    op.press(vec![ctrl('p')]);
    assert!(op.app.jump.is_some(), "⌃p stands");
    op.press(vec![key(KeyCode::Esc), ctrl('b')]);
    assert!(op.app.jump.is_none(), "⌃b does nothing");
    world.close();
}

#[test]
fn a_page_of_a_pack_or_the_operator_has_a_key_that_sends_its_own_intent() {
    let d = Dirs::new("keys-page");
    put(&d, "ui/pages.toml", SIDE);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":side");
    op.until("the side page", |s| !crate::screen_drive::on_chat(s));
    op.press(vec![key(KeyCode::Char('x'))]);
    op.until("the chat again, where the page's key sent us", crate::screen_drive::on_chat);
    world.close();
}
