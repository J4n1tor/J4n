//! Every `[[command]]` the real core has is reached from `⌃k` on the real screen. The front and the screen are wired as the
//! binary wires them; for each row of the palette the jump is opened, the command's first word is typed, the command is among what it
//! lists, and `⏎` does it — says the word where the answer comes, or leaves it in the composer when it wants more.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n_proto::PALETTE;
use j4n_tui::headless::{Headless, KeyCode, ctrl, press as key, typed};

#[test]
fn every_command_of_the_palette_is_found_with_ctrl_k_and_enter_does_it() {
    let d = Dirs::new("jump-commands");
    let world = crate::ui_keys::open(&d);
    let mut op = Operator::sit(&world);
    // `:pack` lists every pack in the chat, a row each, and the screen shows the tail: tall enough for the list to keep its first line (the height grows with every pack)
    op.screen = Headless::new(160, 56);
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !op.app.rows.get(PALETTE).is_some_and(|r| !r.is_empty()) {
        assert!(std::time::Instant::now() < end, "the core never sent the palette");
        op.until("what the core sends", |_| true);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let palette = op.app.rows[PALETTE].clone();
    assert!(palette.len() >= 8, "the base pack has its commands: {:?}", palette.iter().map(|r| &r.key).collect::<Vec<_>>());
    for row in palette {
        let words = row.cells[0].text.clone();
        let word = words.split_whitespace().next().expect("a command has a word").to_string();
        let takes_more = !row.cells[1].text.is_empty();
        op.press(vec![key(KeyCode::Esc)]);
        op.press(vec![ctrl('k')]);
        op.press(typed(&word, false));
        let hits = op.app.jump.as_ref().expect("the jump is open").hits.clone();
        let at = hits.iter().position(|h| h.command && h.id == row.key).unwrap_or_else(|| panic!("`{word}` is not found in the jump: {hits:?}"));
        assert_eq!(hits[at].title, words, "the jump shows the command by its words");
        // the one the jump has under the cursor is the one picked
        for _ in 0..at {
            op.press(vec![key(KeyCode::Down)]);
        }
        op.press(vec![key(KeyCode::Enter)]);
        assert!(op.app.jump.is_none(), "`{word}`: ⏎ closes the jump");
        if takes_more {
            assert_eq!(op.app.line.buf, format!("{word} "), "`{word}` wants more: its word waits in the composer");
            op.press(vec![ctrl('u')]);
        } else {
            let seen = op.until(&format!("`{word}` done"), |s| s.contains(&word) || !crate::screen_drive::on_chat(s));
            assert!(op.app.line.buf.is_empty(), "`{word}` is said, not left to be typed: {seen}");
        }
    }
    world.close();
}
