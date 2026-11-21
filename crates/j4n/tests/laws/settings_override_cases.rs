//! The operator's one-row override of a setting in `~/.j4n` takes effect in the binary's own wiring, and one that cannot is said
//! at start with its file and its id.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use std::fs;

fn open(d: &Dirs, row: &str) -> World {
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/mine.toml"), format!("api = 1\n{row}")).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: None, model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    wire::open(setup, &opts, false, "main").unwrap_or_else(|e| panic!("opens: {e}"))
}

#[test]
fn a_one_row_override_in_the_home_layer_is_the_value_the_loop_reads() {
    let d = Dirs::new("settings-override-fits");
    let w = open(&d, "[[setting]]\nid = \"compact.at\"\nvalue = 0.1\n");
    assert_eq!(w.front.cfg().compact_at, 0.1);
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    w.close();
}

#[test]
fn a_one_row_override_that_does_not_fit_leaves_the_default_and_the_start_says_which_file_and_which_id() {
    let d = Dirs::new("settings-override-bad");
    let w = open(&d, "[[setting]]\nid = \"compact.at\"\nvalue = 0.01\n[[setting]]\nid = \"compact.nope\"\nvalue = 1\n");
    assert_eq!(w.front.cfg().compact_at, 0.7);
    let said = w.notices.join("\n");
    assert!(said.contains("home:settings/mine.toml [[setting]] compact.at") && said.contains("0.01 is below 0.1"), "{said}");
    assert!(said.contains("home:settings/mine.toml [[setting]] compact.nope") && said.contains("no layer defines it"), "{said}");
    w.close();
}
