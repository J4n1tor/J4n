//! A settings file of the operator's own layers that lacks `api` is read as the current api and the start says so; a file with
//! an api this J4n cannot read is left out and the start names the file and what it lacks.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use std::fs;

fn open(d: &Dirs, text: &str) -> World {
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/mine.toml"), text).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: None, model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: j4n_core::task::Mode::Write, place: None, shape: false, anywhere: false };
    wire::open(setup, &opts, false, "main").unwrap_or_else(|e| panic!("opens: {e}"))
}

#[test]
fn a_home_settings_file_without_api_is_read_and_the_start_says_it_was_assumed() {
    let d = Dirs::new("settings-api-assumed");
    let w = open(&d, "[[setting]]\nid = \"compact.at\"\nvalue = 0.1\n");
    assert_eq!(w.front.cfg().compact_at, 0.1, "the row is taken");
    let said = w.notices.join("\n");
    assert!(said.contains("home:settings/mine.toml: has no `api`, read as `api = 1`"), "{said}");
    w.close();
}

#[test]
fn a_home_settings_file_with_an_api_this_j4n_cannot_read_is_left_out_and_the_start_names_the_file() {
    let d = Dirs::new("settings-api-left-out");
    let w = open(&d, "api = 0\n[[setting]]\nid = \"compact.at\"\nvalue = 0.1\n");
    assert_eq!(w.front.cfg().compact_at, 0.7, "the row is not taken");
    let said = w.notices.join("\n");
    assert!(said.contains("home:settings/mine.toml: needs `api = 1` at the top (it has `api = 0`), so it is left out"), "{said}");
    w.close();
}
