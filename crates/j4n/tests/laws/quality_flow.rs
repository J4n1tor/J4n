//! The pick of a doer under `crew.goal = quality`, through the real binary: the settings of the home turn the goal on, the seeds of the pack are read, and the
//! rank that answers `who:work` is the one that weighs the record. With the goal at `cost`, or the switch off, the rank is the one it always was.

use crate::run_cases::{Dirs, j4n};
use serde_json::{Value, json};
use std::fs;

fn home(d: &Dirs, settings: &str) {
    let file = d.j4n_home().join("settings/crew.toml");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, format!("api = 1\n[[enable]]\nid = \"crew\"\n{settings}")).unwrap();
}

/// What the rank says of the two doers J4n has for the board and one it has not (`aaa`, the smallest id: it wins every tie of numbers), as one reply.
fn asked(d: &Dirs) -> Value {
    let line = json!({ "id": "q", "point": "who:work", "slices": { "line": "fix the parser", "thread": "new" }, "offered": ["aaa", "claude", "codex"] }).to_string();
    let out = j4n(d, &["answer", "--by", "rank"], Some(&format!("{line}\n")), &[]);
    assert_eq!(out.rc, 0, "{}", out.err);
    serde_json::from_str(out.out.lines().next().unwrap_or_default()).unwrap_or_else(|e| panic!("{}: {e}", out.out))
}

fn setting(id: &str, value: &str) -> String {
    format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n")
}

#[test]
fn the_goal_in_the_settings_chooses_the_rank_the_board_stands_in_for_a_record_and_cost_leaves_the_rank_as_it_was() {
    // quality, with the switch on: the rank weighs the record's number, which starts from the board (claude 63.9 against codex 60.9, three points apart)
    let d = Dirs::bare("quality-on");
    home(&d, &[setting("crew.on", "true"), setting("crew.goal", "\"quality\"")].concat());
    let got = asked(&d);
    let text = got.to_string();
    assert!(text.contains("weights: quality 1") && !text.contains("reliability"), "{text}");
    assert!(text.contains("no clear lead"), "the board's gap is inside its spread: {text}");
    // with no seeds read the three would tie and `aaa` would win; the board puts claude ahead of codex and a doer the board does not know last
    assert_eq!(got["answer"], "claude", "{text}");
    // the goal is cost: the rank of the base pack
    for settings in [[setting("crew.on", "true"), setting("crew.goal", "\"cost\"")].concat(), setting("crew.on", "true"), [setting("crew.goal", "\"quality\"")].concat()] {
        let d = Dirs::bare("quality-off");
        home(&d, &settings);
        let text = asked(&d).to_string();
        assert!(text.contains("reliability") && !text.contains("weights: quality"), "{settings}\n{text}");
    }
}
