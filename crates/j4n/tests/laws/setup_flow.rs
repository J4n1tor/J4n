//! Agents › Setup: the steps to a working agent, a model and a Decider, driven with keys on the real wire, front, Serve and TUI. A
//! fresh J4n home (J4n basic: the Decider's pack and the accounts are off) and a stand-in `claude` outside HOME that is found and
//! signed in: the Agent step says so; `⏎` on it uses it (`lane.doer`) and `u` takes it back; the model pick sets `lane.model` and its
//! `default` leaves it empty; the Decider turned on from its row is on for every project, and the row `this project only` for this
//! one; no text of the page names a shell command, a path or a file; and the page is its goldens at 160, 120 and 60 columns. The Test
//! step's proof call, with the `test` doer and the test answerer (`deciders_flow`'s gateway on the loopback, which is Jev): each part says
//! ok from what its run wrote, with its time and its cost; a part that failed is the row that mends it; the step is done only from a pass.

use crate::deciders_flow::{Cases, events, gateway, learn};
use crate::models_cases::mock::Mock;
use crate::run_cases::Dirs;
use crate::screen_drive::Operator;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Fold, Row, Up};
use j4n_tui::headless::{KeyCode, press, typed};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A `claude` outside HOME that answers `--version` and `auth status`, signed in or not.
fn claude(d: &Dirs, signed: bool) -> String {
    let p = d.dir.join("bin/claude");
    script(&p, &format!("case \"$1\" in --version) echo '2.1.0 (Claude Code)';; auth) echo '{{\"loggedIn\":{signed},\"authMethod\":\"claude.ai\"}}';; esac"));
    p.display().to_string()
}

fn open_at(d: &Dirs, root: &Path, claude: &str) -> World {
    open_as(d, root, claude, Some("test"))
}

/// The world with the launch's pin `doer`; none: a line's doer is elected, the operator's pick first.
fn open_as(d: &Dirs, root: &Path, claude: &str, doer: Option<&str>) -> World {
    open_pinned(d, root, claude, doer, None)
}

/// [`open_as`], with the launch's `--model` too.
fn open_pinned(d: &Dirs, root: &Path, claude: &str, doer: Option<&str>, model: Option<&str>) -> World {
    let opts = Opts { doer: doer.map(String::from), model: model.map(String::from), approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: root.to_path_buf(), home: d.j4n_home(), claude: claude.into() };
    let w = crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"));
    answered(&w);
    w
}

/// The programs' probes have answered, as they have by the time an operator reads the page: these cases are about what the rows say, and
/// the rows of a look that is still out (`checking…`) are the core's own case (`setup_check_cases`).
fn answered(w: &World) {
    if let Some(c) = w.front.cfg().setup.as_ref() {
        c.report_now(w.front.main_pick());
    }
}

/// The world with no pin of the launch: the page is about the program on the PATH, as an operator who starts J4n bare has it.
fn open(d: &Dirs, claude: &str) -> World {
    open_as(d, &d.project(), claude, None)
}

/// The rows of the steps, as the core sends them.
fn steps(world: &World) -> Vec<Row> {
    answered(world);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: "view:setup-flow".into() });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

fn says(row: &Row) -> String {
    row.cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" | ")
}

fn step(world: &World, key: &str) -> String {
    steps(world).iter().find(|r| r.key == key).map(says).unwrap_or_else(|| panic!("no row {key}: {:#?}", steps(world).iter().map(|r| (r.key.clone(), says(r))).collect::<Vec<_>>()))
}

/// A setting as the settings hold it now.
fn setting(world: &World, id: &str) -> String {
    match world.front.screen().set.pointer(&format!("/{}", id.replace('.', "/"))) {
        j4n_core::value::Value::Str(s) => s,
        other => format!("{other:?}"),
    }
}

fn wait(what: &str, op: &mut Operator, ok: impl Fn() -> bool) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !ok() {
        assert!(std::time::Instant::now() < end, "never saw {what}; the screen is\n{}", op.look());
        op.until("a moment", |_| true);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The line the cursor is on (the focus mark at its start).
pub fn focused(screen: &str) -> String {
    screen.lines().find(|l| l.starts_with('┃')).unwrap_or_default().to_string()
}

/// The cursor moved to the first row from the top whose line holds `what`.
pub fn to_row(op: &mut Operator, what: &str) {
    op.press((0..40).map(|_| press(KeyCode::Up)).collect());
    for _ in 0..40 {
        if focused(&op.look()).contains(what) {
            return;
        }
        op.press(vec![press(KeyCode::Down)]);
    }
    panic!("no row holds {what}:\n{}", op.look());
}

pub fn setup_page(op: &mut Operator) -> String {
    op.go(":setup", "Agents › Setup", |s| s.contains("Agent") && s.contains("Decider") && s.contains("Custom"))
}

#[test]
fn a_fresh_home_with_claude_on_the_path_shows_the_agent_ready_and_enter_uses_it_and_u_takes_it_back() {
    let d = Dirs::bare("setup-agent");
    let w = open(&d, &claude(&d, true));
    let mut op = Operator::sit(&w);
    let s = setup_page(&mut op);
    assert!(s.lines().any(|l| l.contains("Agent") && l.contains("claude · signed in")), "the Agent step says the agent that works:\n{s}");
    assert!(step(&w, "Agent").contains("claude · signed in"), "{}", step(&w, "Agent"));
    assert!(steps(&w).iter().find(|r| r.key == "Agent").is_some_and(|r| r.cells[0].icon == "yes"), "the step is marked done");
    // the step is done, so it is one line: ⏎ opens it to its agents, the one that works first and recommended
    to_row(&mut op, "Agent");
    op.press(vec![press(KeyCode::Enter)]);
    let s = op.until("the agents", |s| s.contains("recommended · ⏎ uses it"));
    assert!(s.lines().any(|l| l.contains("pi") && l.contains("not on this machine")), "{s}");
    to_row(&mut op, "recommended");
    assert_eq!(setting(&w, "lane.doer"), "");
    op.press(vec![press(KeyCode::Enter)]);
    wait("lane.doer set to claude", &mut op, || setting(&w, "lane.doer") == "claude");
    let s = op.until("claude in use", |s| s.contains("in use"));
    assert!(step(&w, "claude").contains("in use"), "{}", step(&w, "claude"));
    // the agent in use carries what can make it work: the API key J4n can give claude
    assert!(s.contains("or use an API key"), "{s}");
    // the Log has the operator's edit, and `u` on the row takes it back
    to_row(&mut op, "in use");
    op.press(vec![press(KeyCode::Char('u'))]);
    wait("lane.doer taken back", &mut op, || setting(&w, "lane.doer").is_empty());
    op.until("claude recommended again", |s| s.contains("recommended · ⏎ uses it"));
    drop(op);
    w.close();
}

#[test]
fn the_model_pick_sets_lane_model_its_default_leaves_it_empty_and_u_takes_it_back() {
    let d = Dirs::bare("setup-model");
    let w = open(&d, &claude(&d, true));
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    assert!(step(&w, "Model").contains("default · the agent picks"), "{}", step(&w, "Model"));
    // ⏎ opens the pick in the row's place: default, then claude's models
    to_row(&mut op, "Model");
    op.press(vec![press(KeyCode::Enter)]);
    let s = op.until("the pick", |s| s.contains("haiku") && s.contains("sonnet"));
    assert!(s.contains("default"), "{s}");
    op.press(vec![press(KeyCode::Right), press(KeyCode::Enter)]);
    wait("lane.model set to haiku", &mut op, || setting(&w, "lane.model") == "haiku");
    op.until("the row says it", |s| focused(s).contains("haiku"));
    // `u` takes it back
    op.press(vec![press(KeyCode::Char('u'))]);
    wait("lane.model taken back", &mut op, || setting(&w, "lane.model").is_empty());
    op.until("the row says the default", |s| s.contains("default · the agent picks"));
    // picked again, then `default`: the setting is left empty, not the word
    to_row(&mut op, "Model");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the pick", |s| s.contains("haiku"));
    op.press(vec![press(KeyCode::Right), press(KeyCode::Right), press(KeyCode::Enter)]);
    wait("lane.model set to sonnet", &mut op, || setting(&w, "lane.model") == "sonnet");
    op.until("the row says sonnet", |s| s.lines().any(|l| l.contains("Model") && l.contains("sonnet")));
    to_row(&mut op, "Model");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the pick", |s| s.contains("haiku"));
    op.press(vec![press(KeyCode::Left), press(KeyCode::Left), press(KeyCode::Enter)]);
    wait("lane.model back to its default", &mut op, || setting(&w, "lane.model").is_empty());
    let file = fs::read_to_string(j4n_core::gate::grant::here_dir(&d.j4n_home(), &d.project()).join("lane.toml")).unwrap_or_default();
    assert!(!file.contains("\"default\""), "the word `default` is no model: {file}");
    assert!(step(&w, "Model").contains("default · the agent picks"), "{}", step(&w, "Model"));
    drop(op);
    w.close();
}

/// A `claude` signed in once its own sign-in has run: `auth login` says a link and leaves a mark, `auth status` reads the mark.
fn claude_to_sign_in(d: &Dirs) -> String {
    let p = d.dir.join("bin/claude");
    let mark = d.dir.join("signed");
    script(&p, &format!("case \"$1 $2\" in --version*) echo '2.1.0 (Claude Code)';; 'auth login') echo 'open https://example.com/login to sign in'; touch '{m}';; 'auth status') if [ -e '{m}' ]; then echo '{{\"loggedIn\":true,\"email\":\"user@example.com\"}}'; else echo '{{\"loggedIn\":false}}'; fi;; esac", m = mark.display()));
    p.display().to_string()
}

#[test]
fn the_agent_in_use_signs_in_from_its_row_without_the_accounts_pack() {
    let d = Dirs::bare("setup-signin");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/lane.toml"), "api = 1\n[[setting]]\nid = \"lane.doer\"\nvalue = \"claude\"\n").unwrap();
    let w = open(&d, &claude_to_sign_in(&d));
    assert!(w.front.screen().packs.iter().any(|p| p.id == "accounts" && !p.on), "J4n basic: no accounts");
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    assert!(step(&w, "Agent").contains("none works"), "{}", step(&w, "Agent"));
    assert!(step(&w, "claude · sign in").contains("not signed in"), "{}", step(&w, "claude · sign in"));
    // ⏎ on its sign-in row asks first, on the card; the yes runs the program's own sign-in, whose link is shown
    to_row(&mut op, "starts its sign-in");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the card", |s| s.contains("sign in to claude with its own login?") && s.contains("y yes · n no"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("claude signed in", &mut op, || d.dir.join("signed").exists());
    crate::screen_drive::wait_idle(&w);
    let said: Vec<String> = w.front.log().read(0).filter(|e| e.kind == "task.settled" || e.kind == "chat.note").map(|e| e.data.to_string()).collect();
    assert!(said.iter().any(|t| t.contains("claude is signed in as user@example.com")), "{said:#?}");
    op.until("the card answered and gone", |s| !s.contains("with its own login?"));
    // `r` on the agent looks again: the Agent step is done, its sign-in row gone
    to_row(&mut op, "in use");
    op.press(vec![press(KeyCode::Char('r'))]);
    wait("the Agent step done", &mut op, || step(&w, "Agent").contains("claude · signed in"));
    assert!(!steps(&w).iter().any(|r| r.key == "claude · sign in"), "the sign-in row is gone");
    drop(op);
    w.close();
}

/// The `enable` row of `pack` in `harness.toml` of `dir`, if any.
fn pack_on(dir: &Path, pack: &str) -> Option<bool> {
    let text = fs::read_to_string(dir.join("harness.toml")).ok()?;
    let t: toml::Table = toml::from_str(&text).ok()?;
    t.get("enable")?.as_array()?.iter().filter_map(toml::Value::as_table).find(|e| e.get("id").and_then(toml::Value::as_str) == Some(pack))?.get("on")?.as_bool()
}

#[test]
fn the_other_keys_of_the_page_do_what_their_rows_say() {
    // claude in use and signed in, an Anthropic key kept: its key row, the row that gives it, the agent whose pack is off, the Decider's none
    let d = Dirs::bare("setup-keys");
    let keys = d.j4n_home().join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    crate::support::key_file(&keys.join("anthropic"), "setup-made-up-anthropic-5a6b7c8d\n");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/lane.toml"), "api = 1\n[[setting]]\nid = \"lane.doer\"\nvalue = \"claude\"\n").unwrap();
    let w = open(&d, &claude(&d, true));
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    // the Agent step is done, so it is shut: ⏎ opens it
    to_row(&mut op, "Agent");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the agents", |s| s.contains("or use an API key"));
    // ⏎ on the key row opens the masked editor in its place, and esc leaves it as it was
    to_row(&mut op, "or use an API key");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the masked editor", |s| s.contains("paste or type the key"));
    op.press(vec![press(KeyCode::Esc)]);
    op.until("the row again", |s| !s.contains("paste or type the key"));
    // ⏎ on `give it the API key` sets `doer.keys`, which acts from the next start, and says so; `u` takes it back
    to_row(&mut op, "give it the key");
    op.press(vec![press(KeyCode::Enter)]);
    wait("doer.keys names claude", &mut op, || setting(&w, "doer.keys") == "claude");
    op.until("the row says it waits", |s| s.lines().any(|l| l.contains("it is given the API key · ⟳ next start")));
    to_row(&mut op, "give it the key");
    op.press(vec![press(KeyCode::Char('u'))]);
    wait("doer.keys taken back", &mut op, || setting(&w, "doer.keys").is_empty());
    // the agent whose pack is off: ⏎ asks on the card, and once the pack is on for every project the row says it waits for the next start
    to_row(&mut op, "its pack is off");
    let pack = steps(&w).into_iter().find(|r| says(r).contains("its pack is off")).map(|r| r.key).unwrap();
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the card", |s| s.contains("widens what J4n may do") && s.contains("y yes"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("its pack on for every project", &mut op, || pack_on(&d.j4n_home(), "agent") == Some(true));
    op.until("the row waits", |s| s.contains("its pack is on from the next start · ⟳"));
    let row = steps(&w).into_iter().find(|r| r.key == pack).unwrap();
    assert!(says(&row).contains("⟳") && !says(&row).contains("turns it on"), "{}", says(&row));
    assert!(!row.actions.iter().any(|a| a.label.contains("turn on")), "no key turns it on again: {:?}", row.actions);
    // the Decider on, then `none` turns it off for every project
    to_row(&mut op, "Span through");
    op.press(vec![press(KeyCode::Char('o'))]);
    op.until("the card", |s| s.contains("widens what J4n may do") && s.contains("y yes"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("the Decider on for every project", &mut op, || pack_on(&d.j4n_home(), "jev") == Some(true));
    op.until("none can be picked", |s| s.contains("⏎ turns the Decider off"));
    to_row(&mut op, "turns the Decider off");
    op.press(vec![press(KeyCode::Enter)]);
    wait("the Decider off for every project", &mut op, || pack_on(&d.j4n_home(), "jev") == Some(false));
    drop(op);
    w.close();
}

/// The `enable` row of the Decider's pack in `harness.toml` of `dir`, if any.
fn decider_on(dir: &Path) -> Option<bool> {
    let text = fs::read_to_string(dir.join("harness.toml")).ok()?;
    let t: toml::Table = toml::from_str(&text).ok()?;
    t.get("enable")?.as_array()?.iter().filter_map(toml::Value::as_table).find(|e| e.get("id").and_then(toml::Value::as_str) == Some("jev"))?.get("on")?.as_bool()
}

#[test]
fn the_decider_turned_on_from_its_row_is_on_for_every_project_and_this_project_only_is_on_here() {
    let d = Dirs::bare("setup-decider");
    let program = claude(&d, true);
    let w = open(&d, &program);
    let here = j4n_core::gate::grant::here_dir(&d.j4n_home(), &d.project());
    let mut op = Operator::sit(&w);
    let s = setup_page(&mut op);
    // not set up: the step is open, Span is recommended (no key at all), and none is what holds now
    assert!(s.contains("Span through") && s.contains("Jev · System One") && s.contains("this project only"), "{s}");
    assert!(step(&w, "Span through OpenRouter").contains("recommended"), "{}", step(&w, "Span through OpenRouter"));
    assert!(step(&w, "Decider").contains("off"), "{}", step(&w, "Decider"));
    // `o` asks on the card, which names what it widens; the yes turns the pack on in the home's own layer
    to_row(&mut op, "Span through");
    op.press(vec![press(KeyCode::Char('o'))]);
    op.until("the card", |s| s.contains("widens what J4n may do") && s.contains("y yes"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("the pack on for every project", &mut op, || decider_on(&d.j4n_home()) == Some(true));
    assert_eq!(decider_on(&here), None, "this project's folder says nothing of it");
    op.until("the step says it waits for the next start", |s| s.contains("on from the next start · ⟳"));
    // the row the others leave: `:undo` takes it back, and `this project only` writes this project's folder alone
    assert_eq!(w.front.say("main", ":undo"), j4n_core::front::Said::Command("undo".into()));
    wait("the pack off again", &mut op, || decider_on(&d.j4n_home()).is_none_or(|on| !on));
    op.until("the page again", |s| s.contains("this project only"));
    to_row(&mut op, "this project only");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the card", |s| s.contains("widens what J4n may do") && s.contains("y yes"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("the pack on here", &mut op, || decider_on(&here) == Some(true));
    assert!(decider_on(&d.j4n_home()).is_none_or(|on| !on), "the home's layer is left as it was");
    drop(op);
    w.close();
    drop(w);
    // the next start of this project has it; another project of the same home does not
    let again = open(&d, &program);
    assert!(again.front.screen().packs.iter().any(|p| p.id == "jev" && p.on), "this project's next start loads it");
    again.close();
    drop(again);
    let other = d.dir.join("other");
    fs::create_dir_all(&other).unwrap();
    let elsewhere = open_at(&d, &other, &program);
    assert!(elsewhere.front.screen().packs.iter().any(|p| p.id == "jev" && !p.on), "another project of the home is left off");
    elsewhere.close();
}

/// The status bar's cells, as the core sends them.
fn status(world: &World) -> String {
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: "bar:status".into() });
    downs.iter().rev().find_map(|d| if let Down::Rows { bind, rows, .. } = d { (bind == "bar:status").then(|| rows.first().map(says).unwrap_or_default()) } else { None }).unwrap_or_default()
}

/// The Decider step and the status bar read one state and say it in the same words: off in a fresh home, on from the next start after
/// `o`, and `⏎` on a Decider whose key is there turns it on (the card asks first).
#[test]
fn the_decider_step_and_the_status_bar_say_the_same_thing() {
    let d = Dirs::bare("setup-decider-bar");
    let w = open(&d, &claude(&d, true));
    assert!(step(&w, "Decider").contains("off ·"), "{}", step(&w, "Decider"));
    assert!(status(&w).contains("Decider off"), "{}", status(&w));
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    to_row(&mut op, "Span through");
    op.press(vec![press(KeyCode::Char('o'))]);
    op.until("the card", |s| s.contains("widens what J4n may do") && s.contains("y yes"));
    op.press(vec![press(KeyCode::Char('y'))]);
    wait("the pack on", &mut op, || decider_on(&d.j4n_home()) == Some(true));
    op.until("the step says it waits for the next start", |s| s.contains("on from the next start · ⟳"));
    assert!(step(&w, "Decider").contains("on from the next start · ⟳"), "{}", step(&w, "Decider"));
    assert!(status(&w).contains("Decider on from the next start ⟳"), "the bar says what the step says: {}", status(&w));
    drop(op);
    w.close();
}

/// The accounts kept in the J4n home are choices under their agent in Agents › Setup, in a project that has no row for them: with the
/// accounts pack off the row turns it on; with it on, `⏎` runs the main lane as the account (one edit: `lane.doer`, `lane.account`, and a
/// copy of the account's row here), the main chat is bound to that login, and the chat's empty state names it before the first line.
#[test]
fn an_account_kept_in_the_home_is_a_choice_under_its_agent_and_enter_runs_the_main_lane_as_it() {
    let d = Dirs::bare("setup-account");
    let program = claude(&d, true);
    let home = d.j4n_home();
    // an account made in another project of this home: its folder, and its row in that project's folder
    fs::create_dir_all(home.join("accounts/claude-1/home")).unwrap();
    fs::create_dir_all(home.join("projects/0123456789abcdef/account")).unwrap();
    fs::write(home.join("projects/0123456789abcdef/account/claude-1.toml"), "api = 1\n\n[[account]]\nid = \"claude-1\"\nconnector = \"claude\"\nplan = \"max5\"\n").unwrap();
    let w = open(&d, &program);
    let row = step(&w, "claude · claude-1");
    assert!(row.contains("its pack, accounts, is off · ⏎ turns it on"), "{row}");
    assert!(steps(&w).iter().find(|r| r.key == "claude · claude-1").is_some_and(|r| r.actions.iter().any(|a| a.risk >= 50)), "turning the pack on asks on the card");
    w.close();
    drop(w);
    // the pack on for the home: the account is a choice, and ⏎ on it picks it
    fs::write(home.join("harness.toml"), "api = 1\n\n[[enable]]\nid = \"accounts\"\non = true\n").unwrap();
    let w = open(&d, &program);
    let here = j4n_core::gate::grant::here_dir(&home, &d.project());
    assert!(step(&w, "claude · claude-1").contains("⏎ runs this project's tasks as it"), "{}", step(&w, "claude · claude-1"));
    assert!(step(&w, "claude · default").contains("its own sign-in"));
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    // the step is done, so it is one line: ⏎ opens it to its agents and their accounts
    to_row(&mut op, "Agent");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the accounts under claude", |s| s.contains("claude · claude-1") && s.contains("claude · default"));
    to_row(&mut op, "claude · claude-1");
    op.press(vec![press(KeyCode::Enter)]);
    wait("lane.account set", &mut op, || setting(&w, "lane.account") == "claude-1");
    assert_eq!(setting(&w, "lane.doer"), "claude");
    let copied = fs::read_to_string(here.join("account/claude-1.toml")).unwrap_or_default();
    assert!(copied.contains("connector = \"claude\"") && copied.contains("max5"), "the account's row is copied here: {copied}");
    // the setting is written first and the main chat is bound right after it, so a loaded machine can be read between
    wait("the main chat bound as the account", &mut op, || {
        w.front.binding("main").is_some_and(|b| b.account.as_deref() == Some("claude-1"))
    });
    let b = w.front.binding("main").expect("the main chat is bound");
    assert_eq!((b.doer.as_str(), b.account.as_deref()), ("claude", Some("claude-1")), "the main chat runs as the account");
    op.until("the row in use", |s| s.lines().any(|l| l.contains("claude · claude-1") && l.contains("in use")));
    // `u` takes the whole pick back
    to_row(&mut op, "claude · claude-1");
    op.press(vec![press(KeyCode::Char('u'))]);
    wait("lane.account taken back", &mut op, || setting(&w, "lane.account").is_empty());
    drop(op);
    w.close();
}

/// What sends the operator out of the TUI: a command, a path, a file, a shell. Lower case.
pub const OUT: [&str; 13] = ["`", "~/", "/home", ".toml", "file:", "env:", "a file", "terminal", "shell", "export ", "by hand", "start j4n again", "j4n doctor"];

fn out_of_tui(text: &str) -> Vec<&'static str> {
    let t = text.to_lowercase();
    OUT.iter().copied().filter(|w| t.contains(w)).collect()
}

#[test]
fn no_text_of_the_setup_page_names_a_shell_command_a_path_or_a_file() {
    // the states that bring out every row: a fresh home; claude picked and not signed in, with an Anthropic key kept, so its sign-in, its key and the row that gives it show
    let d = Dirs::bare("setup-words");
    let w = open(&d, &claude(&d, true));
    let mut found: Vec<String> = Vec::new();
    let mut check = |what: &str, text: &str| found.extend(out_of_tui(text).into_iter().map(|w| format!("{what}: `{w}` in {text}")));
    for r in steps(&w) {
        check("fresh", &says(&r));
    }
    w.close();
    drop(w);
    let d = Dirs::bare("setup-words-picked");
    let keys = d.j4n_home().join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    crate::support::key_file(&keys.join("anthropic"), "setup-made-up-anthropic-0c1d2e3f\n");
    fs::create_dir_all(d.j4n_home().join("settings")).unwrap();
    fs::write(d.j4n_home().join("settings/lane.toml"), "api = 1\n[[setting]]\nid = \"lane.doer\"\nvalue = \"claude\"\n").unwrap();
    let w = open(&d, &claude(&d, false));
    let rows = steps(&w);
    let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    for want in ["claude · sign in", "claude · API key", "claude · give the key"] {
        assert!(keys.contains(&want), "the row {want} is there: {keys:?}");
    }
    assert!(step(&w, "claude · API key").contains("set · J4n file"), "{}", step(&w, "claude · API key"));
    for r in &rows {
        check("claude picked", &says(r));
    }
    // and every frame of the page, each step open, at 60 columns, where words are cut
    let mut op = Operator::sit(&w);
    op.screen = j4n_tui::headless::Headless::new(60, 30);
    op.up.send(Up::Resize { cols: 60, rows: 30 }).unwrap();
    let s = setup_page(&mut op);
    let page: String = s.lines().skip(4).collect::<Vec<_>>().join("\n");
    check("the page at 60 columns", &page);
    drop(op);
    w.close();
    assert!(found.is_empty(), "texts of Agents › Setup that send the operator out of the TUI:\n{}", found.join("\n"));
}

// ---- the Test step: the proof call ----

/// A key of no vendor's shape, which the gateway takes, and one it refuses.
const RIGHT: &str = "setup-right-7c41e09b2d5f";
const WRONG: &str = "setup-wrong-3a9f62d18e04";

/// Jev at the loopback, taking `RIGHT` (one field of the operator's own over the pack's row), and `key` kept for TypeSafe as a key typed in
/// Agents › Providers keeps it: for the provider and its connector.
fn jev_with(d: &Dirs, cases: &Cases, key: &str) -> Mock {
    let jev = gateway(cases, RIGHT);
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/jev.toml"), format!("api = 1\n[[connector]]\nid = \"jev\"\nurl = \"{}\"\n", jev.url())).unwrap();
    let keys = d.j4n_home().join("keys");
    fs::create_dir_all(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    for id in ["typesafe", "jev"] {
        crate::support::key_file(&keys.join(id), key);
    }
    jev
}

/// The last proof call the Log holds.
fn tested(world: &World) -> Option<serde_json::Value> {
    events(world).into_iter().rev().find(|(_, k, _)| k == "setup.tested").map(|(_, _, d)| d)
}

fn row_of(world: &World, key: &str) -> Row {
    steps(world).into_iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key}"))
}

/// The proof call made with `⏎` on the Test row (or on `run again`), and what the Log says of it.
pub fn run_test(op: &mut Operator, w: &World, on: &str) -> serde_json::Value {
    let before = events(w).iter().filter(|(_, k, _)| k == "setup.tested").count();
    to_row(op, on);
    op.press(vec![press(KeyCode::Enter)]);
    wait("the test's end", op, || events(w).iter().filter(|(_, k, _)| k == "setup.tested").count() > before);
    let t = tested(w).unwrap();
    // the screen is drawn from the core's rows: the step says how it went
    op.until("the step says how it went", |s| s.lines().any(|l| l.contains("Test") && (l.contains("passed ·") || l.contains("failed ·"))));
    t
}

#[test]
fn the_test_says_ok_per_part_from_what_its_runs_wrote_with_time_and_cost_and_done_comes_only_from_a_pass() {
    let d = Dirs::new("setup-pass");
    let cases: Cases = Arc::default();
    let jev = jev_with(&d, &cases, RIGHT);
    // started with `--doer test`: the proof's turn is the scripted doer's
    let w = open_at(&d, &d.project(), &claude(&d, true));
    learn(&w, &cases);
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    // not run: the step is open, not done, and its ⏎ runs it
    assert!(step(&w, "Test").contains("not run") && step(&w, "Test").contains("⏎ runs it"), "{}", step(&w, "Test"));
    let r = row_of(&w, "Test");
    assert!(r.cells[0].icon != "yes" && r.fold != Some(Fold::Folded), "not done before a test: {r:?}");
    assert!(tested(&w).is_none());
    let asked = jev.seen().len();
    let t = run_test(&mut op, &w, "not run");
    assert_eq!(t["passed"], true, "{t:#}");
    // each part from what its run wrote: the agent's turn (its task's events), the Decider's cases (its `probe.result`)
    let (agent, decider) = (&t["parts"][0], &t["parts"][1]);
    assert_eq!((agent["part"].as_str(), agent["id"].as_str(), agent["ok"].as_bool()), (Some("agent"), Some("test"), Some(true)), "{t:#}");
    let task = agent["task"].as_str().unwrap();
    assert!(w.front.log().read(0).any(|e| e.task.as_deref() == Some(task) && e.kind.starts_with("run.")), "the turn ran and said so");
    let turn = w.front.tasks().book().get(task).unwrap();
    assert!(turn.state == j4n_core::task::State::Done { ok: true } && turn.spec.thread.as_deref() == Some("setup-test") && turn.spec.tune.mode == Mode::Read, "a turn that ended well, on its own thread, in read mode: {:?}", turn.state);
    let log = events(&w);
    assert_eq!((decider["id"].as_str(), decider["ok"].as_bool()), (Some("decider:jev"), Some(true)), "{t:#}");
    // the run at start puts the Decider's other points beside the test's, each its own `probe.result`: the part's is the one of its point
    let point = decider["point"].as_str().unwrap_or_else(|| panic!("the part names its point: {t:#}"));
    let results: Vec<String> = log.iter().filter(|(_, k, d)| k == "probe.result" && d["answerer"] == "decider:jev").map(|(_, _, d)| format!("{} {}", d["point"], d["usd"])).collect();
    let probe = log.iter().rev().find(|(_, k, d)| k == "probe.result" && d["answerer"] == "decider:jev" && d["point"] == point).map(|(_, _, d)| d.clone()).unwrap_or_else(|| panic!("no probe.result at {point}: {results:?}"));
    assert!(probe["passed"] == true && probe["usd"].as_f64().is_some_and(|u| u > 0.0), "the cases' cost, as the gateway said it: {probe}");
    assert_eq!(decider["usd"], probe["usd"], "the part's cost is its point's ({point}); the Decider's results: {results:?}");
    assert!(jev.seen().len() > asked, "the Decider was asked by the test");
    // the rows say it: ok, the time, and the cost where the part said one (the `test` doer says none, so none is shown)
    let a = step(&w, "Test · agent");
    assert!(a.starts_with("test") && a.contains("ok · ") && a.contains(" s") && !a.contains('$'), "{a}");
    let j = step(&w, "Test · decider");
    assert!(j.starts_with("Jev · System One") && j.contains("ok · ") && j.contains(" s · $0.000"), "{j}");
    // done: the step is marked and shut, as a done step is; ⏎ opens it to its parts and the row that runs it again
    let r = row_of(&w, "Test");
    assert!(r.cells[0].icon == "yes" && r.fold == Some(Fold::Folded) && says(&r).contains("passed ·"), "{r:?}");
    to_row(&mut op, "passed ·");
    op.press(vec![press(KeyCode::Enter)]);
    let s = op.until("the parts", |s| s.contains("run again") && s.contains("Jev · System One") && s.contains("ok · "));
    assert!(s.lines().any(|l| l.contains("test") && l.contains("ok · ")), "{s}");
    drop(op);
    w.close();
}

/// A `claude` that plays a recorded turn (its answer, its cost) for each line it is sent, and answers `--version` and `auth status`.
fn claude_playing(d: &Dirs) -> String {
    let real = crate::run_cases::program(d, "claude-real", &crate::support::read(&crate::support::root().join("fixtures/claude-frames/fake-claude.sh")));
    let two = crate::support::root().join("fixtures/claude-frames/two-turns.jsonl");
    crate::run_cases::program(d, "claude", &format!("#!/bin/sh\nFAKE_FIXTURE='{}' exec '{}' \"$@\"\n", two.display(), real.display())).display().to_string()
}

#[test]
fn a_part_that_failed_is_the_row_that_mends_it_and_the_step_is_done_only_once_a_test_passed() {
    // claude picked, playing a turn that costs a cent; Jev given a key its gateway refuses
    let d = Dirs::new("setup-mend");
    fs::write(d.j4n_home().join("settings/lane.toml"), "api = 1\n[[setting]]\nid = \"lane.doer\"\nvalue = \"claude\"\n").unwrap();
    let cases: Cases = Arc::default();
    let _jev = jev_with(&d, &cases, WRONG);
    let w = open_as(&d, &d.project(), &claude_playing(&d), None);
    learn(&w, &cases);
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    let t = run_test(&mut op, &w, "not run");
    assert_eq!((t["passed"].as_bool(), t["parts"][0]["ok"].as_bool(), t["parts"][1]["ok"].as_bool()), (Some(false), Some(true), Some(false)), "{t:#}");
    // the agent's cost is its turn's, from its `run.metrics`
    let task = t["parts"][0]["task"].as_str().unwrap().to_string();
    assert!(w.front.log().read(0).any(|e| e.kind == "run.metrics" && e.task.as_deref() == Some(task.as_str()) && e.data["usd"] == 0.01), "the turn's cost was written");
    let a = step(&w, "Test · agent");
    assert!(a.starts_with("claude") && a.contains("ok · ") && a.contains(" s · $0.0100"), "{a}");
    // not done: the step says what failed, and the Decider's row is its key's row, the masked editor on its ⏎
    let r = row_of(&w, "Test");
    assert!(r.cells[0].icon == "no" && r.fold != Some(Fold::Folded) && says(&r).contains("failed · Decider"), "{r:?}");
    let j = step(&w, "Test · decider");
    assert!(j.contains("failed · ") && j.contains("probe cases") && j.contains("⏎ types its key again"), "{j}");
    assert!(row_of(&w, "Test · decider").actions.iter().any(|a| a.label == "type the key"), "its ⏎ types the key");
    // no text of the failed test sends the operator out of the TUI, nor holds the key
    let words: Vec<String> = steps(&w).iter().map(says).collect();
    let out: Vec<String> = words.iter().flat_map(|t| out_of_tui(t).into_iter().map(move |w| format!("`{w}` in {t}"))).collect();
    assert!(out.is_empty(), "{out:#?}");
    assert!(words.iter().all(|t| !t.contains(&WRONG[..10])), "{words:#?}");
    // the mend: ⏎ on the failed part types the right key in its place, and the test run again passes
    to_row(&mut op, "types its key again");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the masked editor", |s| s.contains("paste or type the key"));
    op.press(typed(RIGHT, false));
    let sets = events(&w).iter().filter(|(_, k, _)| k == "key.set").count();
    op.press(vec![press(KeyCode::Enter)]);
    wait("the key kept", &mut op, || events(&w).iter().filter(|(_, k, _)| k == "key.set").count() > sets);
    std::thread::sleep(std::time::Duration::from_millis(80));
    let t = run_test(&mut op, &w, "run again");
    assert_eq!(t["passed"], true, "{t:#}");
    let r = row_of(&w, "Test");
    assert!(r.cells[0].icon == "yes" && says(&r).contains("passed ·"), "{r:?}");
    // a test of another model proves nothing of what is set now: the step is not done once the pick moves
    op.up.send(Up::Intent { intent: j4n_proto::Intent::Set { kind: "setting".into(), id: "lane.model".into(), field: "value".into(), value: serde_json::json!("haiku") } }).unwrap();
    wait("the step not done again", &mut op, || step(&w, "Test").contains("not run for what is set now"));
    assert_ne!(row_of(&w, "Test").cells[0].icon, "yes");
    op.until("the page says it", |s| s.contains("not run for what is set now") && s.contains("run again"));
    // run again, on that model: the turn is given it, and the step is done again
    let t = run_test(&mut op, &w, "run again");
    assert_eq!((t["passed"].as_bool(), t["model"].as_str()), (Some(true), Some("haiku")), "{t:#}");
    let turn = w.front.tasks().book().get(t["parts"][0]["task"].as_str().unwrap()).unwrap();
    assert_eq!(turn.spec.tune.model.as_deref(), Some("haiku"));
    assert!(step(&w, "Test · agent").starts_with("claude · haiku") && row_of(&w, "Test").cells[0].icon == "yes", "{}", step(&w, "Test · agent"));
    let frames = op.look();
    assert!(!frames.contains(&RIGHT[..10]) && !frames.contains(&WRONG[..10]));
    drop(op);
    w.close();
}

/// The Setup page drawn at each size, the Decider open, as its goldens: `J4N_BLESS=1` writes them.
#[test]
fn the_setup_page_at_160_120_and_60_columns_is_its_goldens() {
    let d = Dirs::bare("setup-golden");
    let w = open(&d, &claude(&d, true));
    let want = crate::support::root().join("crates/j4n/tests/setup");
    let bless = std::env::var_os("J4N_BLESS").is_some_and(|v| !v.is_empty());
    fs::create_dir_all(&want).unwrap();
    let mut found = Vec::new();
    for (cols, rows) in [(160u16, 44u16), (120, 36), (60, 24)] {
        let mut sit = crate::sweep::Sitting::new(&w, (cols, rows));
        sit.line(":setup");
        let got = crate::sweep::steady(&sit.until("Agents › Setup", |s| s.contains("Agent") && s.contains("Custom")));
        let path = want.join(format!("setup.{cols}x{rows}.txt"));
        if bless {
            fs::write(&path, &got).unwrap();
        } else if fs::read_to_string(&path).ok().as_deref() != Some(got.as_str()) {
            found.push(format!("{}: not its golden; J4N_BLESS=1 writes it\n{got}", path.display()));
        }
    }
    w.close();
    crate::support::assert_clean("the Setup page's goldens", &found);
}

/// Started with `--doer test`, every line runs on `test`: the Agent step names it, its row is the one in use and says it was pinned at
/// start. `⏎ use` on another agent still sets `lane.doer`, and that row says it takes effect when J4n starts without the pin; `u` takes it
/// back; a start without the pin uses the pick.
#[test]
fn started_with_a_pinned_doer_the_agent_step_names_it_and_a_pick_waits_for_a_start_without_the_pin() {
    let d = Dirs::bare("setup-pin");
    let program = claude(&d, true);
    let w = open_at(&d, &d.project(), &program);
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    assert!(step(&w, "Agent").contains("test · ready · pinned at start"), "the agent lines run on: {}", step(&w, "Agent"));
    let pinned = row_of(&w, "test");
    assert!(says(&pinned).contains("in use · pinned at start") && pinned.cells[0].icon == "radio-on", "{}", says(&pinned));
    assert!(!pinned.actions.iter().any(|a| a.label == "use"), "the one in use has no use: {:?}", pinned.actions);
    assert!(says(&row_of(&w, "claude")).contains("⏎ uses it") && row_of(&w, "claude").cells[0].icon == "radio-off", "{}", says(&row_of(&w, "claude")));
    // ⏎ use on claude: the pick is kept, and lines still run on the pin
    to_row(&mut op, "Agent");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the agents", |s| s.contains("pinned at start") && s.contains("⏎ uses it"));
    to_row(&mut op, "⏎ uses it");
    op.press(vec![press(KeyCode::Enter)]);
    wait("lane.doer set to claude", &mut op, || setting(&w, "lane.doer") == "claude");
    op.until("the pick waits for a start without the pin", |s| s.contains("it takes effect when J4n starts without the pin"));
    assert!(says(&row_of(&w, "claude")).contains("your pick · it takes effect when J4n starts without the pin"), "{}", says(&row_of(&w, "claude")));
    assert!(says(&row_of(&w, "test")).contains("in use · pinned at start") && step(&w, "Agent").starts_with("Agent | test"), "{}", step(&w, "Agent"));
    assert!(!row_of(&w, "claude").actions.iter().any(|a| a.label == "use"), "picked already: no second use");
    // `u` on the pick takes it back
    to_row(&mut op, "your pick");
    op.press(vec![press(KeyCode::Char('u'))]);
    wait("lane.doer taken back", &mut op, || setting(&w, "lane.doer").is_empty());
    op.until("claude to use again", |s| s.lines().any(|l| l.contains("claude") && l.contains("⏎ uses it")));
    // picked again, then a start without the pin: the pick is in use
    to_row(&mut op, "⏎ uses it");
    op.press(vec![press(KeyCode::Enter)]);
    wait("lane.doer set to claude again", &mut op, || setting(&w, "lane.doer") == "claude");
    drop(op);
    w.close();
    drop(w);
    let w = open(&d, &program);
    assert!(step(&w, "Agent").contains("claude · signed in") && !step(&w, "Agent").contains("pinned"), "{}", step(&w, "Agent"));
    assert!(says(&row_of(&w, "claude")).contains("in use") && !steps(&w).iter().any(|r| r.key == "test"), "{}", says(&row_of(&w, "claude")));
    w.close();
}

/// Started with `--doer claude --model opus`, lines run on opus: the Model step says so, pinned at start. A pick still sets `lane.model`,
/// and the row says it takes effect when J4n starts without the pin; under the agent's pin alone lines run on its own default, and a start
/// without the pin uses the pick.
#[test]
fn started_with_a_pinned_model_the_model_step_names_it_and_a_pick_waits_for_a_start_without_the_pin() {
    let d = Dirs::bare("pin-model");
    let program = claude(&d, true);
    let w = open_pinned(&d, &d.project(), &program, Some("claude"), Some("opus"));
    let mut op = Operator::sit(&w);
    setup_page(&mut op);
    assert!(step(&w, "Model").contains("Model | opus · pinned at start | ⏎ pick"), "the model lines run on: {}", step(&w, "Model"));
    to_row(&mut op, "Model");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the pick", |s| s.contains("haiku") && s.contains("sonnet"));
    op.press(vec![press(KeyCode::Right), press(KeyCode::Enter)]);
    wait("lane.model set to haiku", &mut op, || setting(&w, "lane.model") == "haiku");
    op.until("the pick waits for a start without the pin", |s| s.contains("it takes effect when J4n starts without the pin"));
    assert!(step(&w, "Model").contains("opus · pinned at start | ⏎ pick · your pick haiku · it takes effect when J4n starts without the pin"), "{}", step(&w, "Model"));
    drop(op);
    w.close();
    drop(w);
    let w = open_pinned(&d, &d.project(), &program, Some("claude"), None);
    assert!(step(&w, "Model").contains("default · the agent picks | ⏎ pick · your pick haiku · it takes effect when J4n starts without the pin"), "the pinned agent's own default: {}", step(&w, "Model"));
    w.close();
    drop(w);
    let w = open(&d, &program);
    assert!(step(&w, "Model").contains("Model | haiku | ⏎ pick"), "{}", step(&w, "Model"));
    w.close();
}

/// With no pin and no agent picked, the election's agent runs on `lane.model` when the launch gave no `--model`: the line, the Model step
/// and the Test say the same model.
#[test]
fn an_elected_agent_runs_on_the_model_picked_for_the_main_lane() {
    let d = Dirs::new("elected-model");
    d.offload_off();
    fs::create_dir_all(d.home().join(".claude")).unwrap();
    let real = crate::run_cases::program(&d, "claude-real", &crate::support::read(&crate::support::root().join("fixtures/claude-frames/fake-claude.sh")));
    let log = d.dir.join("claude-log.txt");
    fs::write(d.j4n_home().join("settings/model.toml"), "api = 1\n[[setting]]\nid = \"lane.model\"\nvalue = \"haiku\"\n").unwrap();
    let fixture = crate::support::root().join("fixtures/claude-frames/two-turns.jsonl");
    let env = [("J4N_CLAUDE", real.to_str().unwrap()), ("FAKE_FIXTURE", fixture.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap())];
    let o = crate::run_cases::j4n(&d, &["run", "say one", "--approval", "full", "--wall", "60s"], None, &env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    let said = fs::read_to_string(&log).unwrap_or_default();
    let turn = said.lines().find(|l| l.starts_with("start") && l.contains("stream-json")).unwrap_or_else(|| panic!("claude was elected and started: {said}\n{}", o.err));
    assert!(turn.contains("--model haiku"), "the line ran on lane.model: {turn}");
    // the Model step says the model the line ran on
    let w = open(&d, &real.display().to_string());
    assert!(step(&w, "Model").contains("Model | haiku | ⏎ pick"), "{}", step(&w, "Model"));
    w.close();
}

/// `lane.model` is a pick made for one agent: when the election falls to another whose own models do not have it, that agent runs on its
/// own default, a chat note says so, and the Model step says the same; a word of its own list it runs on.
#[test]
fn an_elected_agent_whose_models_lack_the_main_lanes_pick_runs_on_its_default_and_a_note_says_so() {
    let d = Dirs::new("elected-other");
    d.offload_off();
    let log = d.dir.join("other-log.txt");
    let prog = d.dir.join("bin/other.sh");
    fs::create_dir_all(prog.parent().unwrap()).unwrap();
    fs::write(&prog, format!("#!/bin/sh\nprintf 'start %s\\n' \"$*\" >> '{}'\necho done\n", log.display())).unwrap();
    fs::set_permissions(&prog, fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
    fs::write(d.j4n_home().join("connectors/other.toml"), format!("api = 1\n[[connector]]\nid = \"other\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\nmodel = [\"--model\", \"{{model}}\"]\nmodels = [\"m1\"]\n", prog.display())).unwrap();
    // no claude: the election falls to the other agent
    let claude = d.dir.join("bin/no-claude");
    let env = [("J4N_CLAUDE", claude.to_str().unwrap())];
    let lane = |m: &str| fs::write(d.j4n_home().join("settings/model.toml"), format!("api = 1\n[[setting]]\nid = \"lane.model\"\nvalue = \"{m}\"\n")).unwrap();
    lane("haiku");
    let o = crate::run_cases::j4n(&d, &["run", "say one", "--approval", "full", "--wall", "60s"], None, &env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    let said = fs::read_to_string(&log).unwrap_or_default();
    assert!(said.starts_with("start ") && !said.contains("--model"), "the other agent ran on its own default: {said}\n{}", o.err);
    assert!(format!("{}{}", o.out, o.err).contains("haiku is not a model of other · it runs on its default"), "{}\n{}", o.out, o.err);
    // a model of its own list it is given
    lane("m1");
    fs::write(&log, "").unwrap();
    let o = crate::run_cases::j4n(&d, &["run", "say one", "--approval", "full", "--wall", "60s"], None, &env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    assert!(fs::read_to_string(&log).unwrap_or_default().contains("--model m1"), "{}", o.err);
}

/// The Model step holds the main lane's pick to the elected agent's own models as the line does: claude elected, `lane.model` a word none of
/// its models is, the step says it runs on its default and why, and claude is started with no `--model`. (The step names only the agents
/// the doctor knows, so the agent of the case above has no Model step of its own.)
#[test]
fn the_model_step_says_the_elected_agent_runs_on_its_default_when_the_pick_is_none_of_its_models() {
    let d = Dirs::new("elected-not-its-own");
    d.offload_off();
    fs::create_dir_all(d.home().join(".claude")).unwrap();
    let real = crate::run_cases::program(&d, "claude-real", &crate::support::read(&crate::support::root().join("fixtures/claude-frames/fake-claude.sh")));
    let log = d.dir.join("claude-log.txt");
    fs::write(d.j4n_home().join("settings/model.toml"), "api = 1\n[[setting]]\nid = \"lane.model\"\nvalue = \"gpt-5\"\n").unwrap();
    let fixture = crate::support::root().join("fixtures/claude-frames/two-turns.jsonl");
    let env = [("J4N_CLAUDE", real.to_str().unwrap()), ("FAKE_FIXTURE", fixture.to_str().unwrap()), ("FAKE_LOG", log.to_str().unwrap())];
    let o = crate::run_cases::j4n(&d, &["run", "say one", "--approval", "full", "--wall", "60s"], None, &env);
    assert_eq!(o.rc, 0, "{}\n{}", o.out, o.err);
    let said = fs::read_to_string(&log).unwrap_or_default();
    let turn = said.lines().find(|l| l.starts_with("start") && l.contains("stream-json")).unwrap_or_else(|| panic!("claude was elected and started: {said}\n{}", o.err));
    assert!(!turn.contains("--model"), "claude ran on its own default: {turn}");
    let w = open(&d, &real.display().to_string());
    assert!(step(&w, "Model").contains("Model | default · the agent picks | ⏎ pick · gpt-5 is not a model of claude · it runs on its default"), "{}", step(&w, "Model"));
    w.close();
}
