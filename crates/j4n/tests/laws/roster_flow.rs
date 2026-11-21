//! The roster and the library (`:roster`, `:library`): pages from data, over the connectors and the models of the Table. `:roster` opens
//! Agents › List; the tools are five sub-tabs on, after the models, the providers, the plans and the trust.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use std::fs;

#[test]
fn the_roster_lists_the_connectors_with_their_roles_and_the_library_the_models_and_where_they_come_from() {
    let d = Dirs::new("roster");
    let here = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(here.join("models")).unwrap();
    fs::write(here.join("models/m.toml"), "api = 1\n[[model]]\nid = \"gpt-oss-20b\"\ndoc = \"The small open model.\"\nconnector = \"own\"\nname = \"openai/gpt-oss-20b\"\ndefault = true\ncontext = 131072\n").unwrap();
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let world = wire::open(Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() }, &opts, true, "main").unwrap();
    let mut op = Operator::sit(&world);
    op.type_line(":roster");
    let row = |s: &str, id: &str, rest: &[&str]| s.lines().any(|l| l.contains(id) && rest.iter().all(|r| l.contains(r)) && !crate::screen_drive::is_you(l));
    let s = op.until("the agents", |s| row(s, "claude", &["claude-stream"]));
    assert!(row(&s, "own", &["openai"]), "{s}");
    assert!(s.contains("plan"), "the roles are shown: {s}");
    op.press(vec![j4n_tui::headless::alt_key(j4n_tui::headless::KeyCode::Right); 5]);
    op.until("the tools", |s| s.lines().nth(3).is_some_and(|l| l.contains("Tools")) && row(s, "builtin", &[]));
    // the Models page, by its word, from the chat
    op.press(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(1))]);
    op.type_line(":library");
    op.until("the library", |s| row(s, "gpt-oss-20b", &["own", "131"]));
    drop(op);
    world.close();
}
