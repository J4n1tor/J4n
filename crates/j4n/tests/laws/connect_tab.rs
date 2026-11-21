//! The Connect tab. The programs the add line lists are found where the connector rows say and a
//! connected one is not listed again; the Lab reads only a run's two files; and the keys of a connection row reach what they should: `t` the
//! canary of the Connect flow (the Gate asking), `r` the Connect flow again on the same program, `o` the confirm and then the switch.
//! The front, the loop, the Gate, the files and the Table are the real ones, wired as the binary wires them; the programs are scripts.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::connect::{find_programs, read_run, read_runs};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use j4n_tui::headless::{KeyCode, KeyEvent, alt, press};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn exe(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn the_add_line_lists_an_executable_of_a_local_bin_and_skips_a_connected_one_j4n_s_own_and_the_shells() {
    let d = Dirs::bare("connect-found");
    let (home, project) = (d.dir.join("home"), d.dir.join("project"));
    for name in ["newagent", "already", "j4n-own", "echo", "stray"] {
        exe(&home.join(".local/bin").join(name), "true");
    }
    // a file that is not an executable, and a hidden one, are not programs
    put(&home.join(".local/bin/notes.txt"), "text");
    exe(&home.join(".local/bin/.hidden"), "true");
    fs::set_permissions(home.join(".local/bin/stray"), fs::Permissions::from_mode(0o644)).unwrap();
    exe(&project.join("node_modules/.bin/linter"), "true");
    exe(&project.join("target/release/built"), "true");
    // a folder of the PATH outside the home and the project is the system's: not listed; one under the home is, by its bare name
    exe(&d.dir.join("usr/bin/ls"), "true");
    exe(&home.join(".local/share/mise/bin/oddtool"), "true");
    let path = vec![d.dir.join("usr/bin"), home.join(".local/share/mise/bin"), home.join(".local/bin")];

    let found = find_programs(&home, &project, &path, &["already".to_string()]);
    let names: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["newagent", "linter", "built", "oddtool"], "the home's folders first, then the project's, then the PATH outside the system's: {found:?}");
    let said = |n: &str| found.iter().find(|p| p.name == n).unwrap().said.clone();
    assert_eq!(said("newagent"), "newagent", "a folder of the PATH gives the bare name");
    assert_eq!(said("linter"), project.join("node_modules/.bin/linter").display().to_string(), "a folder that is not on the PATH gives the path");
    assert_eq!(found[0].at, "~/.local/bin", "the home is written ~");
    // none of the skipped ones came back
    for skipped in ["already", "j4n-own", "echo", "stray", "notes.txt", ".hidden", "ls"] {
        assert!(!names.contains(&skipped), "{skipped} is not a program to connect: {names:?}");
    }
}

#[test]
fn the_lab_reads_a_run_from_its_rows_and_its_table_and_nothing_else_of_it() {
    let d = Dirs::bare("connect-lab-read");
    let run = d.dir.join("lab/run-a");
    let row = |seed: u64, held: &str, check: &str, result: &str| format!("{{\"axes\":{{}},\"check\":\"{check}\",\"drafter\":\"haiku\",\"held\":[{held}],\"result\":\"{result}\",\"run\":\"run-a\",\"seed\":{seed},\"why\":\"\"}}\n");
    let rows: String = [(0, "", "pass", "pass"), (1, "", "pass", "fail"), (2, "\"c:server\"", "pass", "pass"), (3, "\"c:server\"", "fail", "fail")].iter().map(|(s, h, a, b)| row(*s, h, "connected", a) + &row(*s, h, "chats", b)).collect();
    put(&run.join("rows.jsonl"), &rows);
    put(&run.join("table.txt"), "lab run run-a: 4 seeds, drafter haiku, spent $0.04\n\naxis value  n\na:env       2\n");
    // a seed's folder holds what the lab ran: it is never read
    put(&run.join("seed-0/secret.txt"), "never");
    let r = read_run(&run).expect("a run");
    assert_eq!((r.seeds, r.drafter.as_str()), (4, "haiku"));
    assert_eq!(r.connected, [(2, 2), (1, 2)], "connected: seen families, then held-out ones");
    assert_eq!(r.chats, [(1, 2), (1, 2)]);
    assert_eq!(r.usd, Some(0.04));
    assert_eq!(r.table.len(), 4);
    // a folder with neither file is no run; two roots are both read, newest first
    put(&d.dir.join("cache/runs/run-b/table.txt"), "lab run run-b: 1 seeds, drafter scripted\n");
    put(&d.dir.join("cache/runs/empty/seed-0/x"), "x");
    let runs = read_runs(&[d.dir.join("lab"), d.dir.join("cache/runs")]);
    let mut names: Vec<&str> = runs.iter().map(|r| r.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["run-a", "run-b"]);
}

// ---- the keys of a connection row, on the real front ----

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"))
}

/// An agent that says `ready`, and a tool that prints what it is given, connected by files of the operator's own folder.
fn furnish(d: &Dirs) -> (PathBuf, PathBuf) {
    let (agent, tool) = (d.dir.join("bin/mytool"), d.dir.join("bin/lint"));
    exe(&agent, "case \"$1\" in --version) echo 'mytool 1.0';; *) echo ready;; esac");
    exe(&tool, "echo \"linted: $1\"");
    put(&d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"connect\"\n");
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n");
    put(
        &here.join("connectors/mytool.toml"),
        &format!("api = 1\n[[connector]]\nid = \"mytool\"\ndoc = \"A small agent.\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", agent.display()),
    );
    put(
        &here.join("connectors/lint.toml"),
        &format!("api = 1\n[[connector]]\nid = \"lint\"\ndoc = \"A linter.\"\nis = \"tool\"\ndialect = \"text\"\nprogram = \"{0}\"\nargs = [{{ name = \"path\", required = true }}]\nargv = [\"{0}\", \"{{path}}\"]\nrisk = \"read\"\ncanary = {{ goal = \"src\", expect = \"linted: src\" }}\n", tool.display()),
    );
    (agent, tool)
}

/// The Connect tab with the cursor on the row of `id`: the line the cursor marks starts with `┃`.
fn on_row(op: &mut Operator, id: &str) -> String {
    op.type_line(":connections");
    op.until("the Connect tab", |s| s.contains(id) && s.contains("connected"));
    for _ in 0..30 {
        let s = op.look();
        if s.lines().any(|l| l.starts_with('┃') && l.contains(id)) {
            return s;
        }
        op.press(vec![press(KeyCode::Down)]);
        std::thread::sleep(Duration::from_millis(30));
    }
    panic!("the cursor never came to {id}:\n{}", op.look());
}

fn events(world: &World, kind: &str) -> Vec<serde_json::Value> {
    world.front.log().sync();
    world.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn queued(world: &World, doer: &str) -> Vec<String> {
    events(world, "task.queued").into_iter().filter(|d| d.pointer("/spec/doer").and_then(|v| v.as_str()) == Some(doer)).map(|d| d.pointer("/spec/goal").and_then(|v| v.as_str()).unwrap_or_default().to_string()).collect()
}

fn wait(what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(Instant::now() < end, "never saw {what}");
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn keys(op: &mut Operator, k: Vec<KeyEvent>) {
    op.press(k);
}

#[test]
fn the_tab_has_a_row_per_connection_with_dashes_where_no_data_is_yet_and_a_page_under_it() {
    let d = Dirs::new("connect-rows");
    let (agent, _) = furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":connections");
    let s = op.until("the rows", |s| s.contains("mytool") && s.contains("lint"));
    let row = |id: &str| s.lines().find(|l| l.contains(id) && !crate::screen_drive::is_you(l) && (l.contains(" agent ") || l.contains(" tool "))).unwrap_or_else(|| panic!("no row of {id}:\n{s}")).to_string();
    // kind, state, limit and last canary: `ok` because the program runs, `—` for the limit and the canary nobody ran
    for (id, kind) in [("mytool", "agent"), ("lint", "tool")] {
        let r = row(id);
        assert!(r.contains(kind) && r.contains(" ok ") && r.matches('—').count() == 2, "{r}");
    }
    // a program that is not there is broken, and a row off says so
    fs::remove_file(&agent).unwrap();
    world.close();
    drop(op);
    drop(world);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":connections");
    let s = op.until("broken", |s| s.lines().any(|l| l.contains("mytool") && l.contains("broken")));
    assert!(s.contains("add"), "{s}");
    world.close();
}

#[test]
fn t_runs_the_canary_through_the_gate_r_connects_again_and_o_asks_first_and_turns_it_off_at_the_next_start() {
    let d = Dirs::new("connect-keys");
    let (agent, _) = furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let s = on_row(&mut op, "mytool");
    // the keys of the row are in the keys row, marked, and the page's sentence of keys is not printed over it
    assert!(!s.contains("on add types a program") && !s.contains("⏎ on a connection opens it"), "{s}");
    assert!(s.lines().any(|l| l.contains("• t test") && l.contains("• r learn again") && l.contains("• o off")), "{s}");

    // t: the canary of the Connect flow on this row's manifest; the Gate asks, as the flow's own canary does
    keys(&mut op, vec![press(KeyCode::Char('t'))]);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    wait("the test flow", || queued(&world, "flow:connect-test").len() == 1);
    let manifest = queued(&world, "flow:connect-test").remove(0);
    assert!(manifest.contains("id = \"mytool\"") && manifest.contains(&agent.display().to_string()), "{manifest}");
    op.until("the leave to run it once", |s| s.contains("connect-canary") && s.contains("y yes"));
    keys(&mut op, vec![alt('y')]);
    wait("the canary to end", || events(&world, "flow.step.ended").iter().any(|e| e["step"] == "canary" && e["ok"] == true));
    // the row says it: pass, and when
    let s = op.until("the last canary", |s| s.lines().any(|l| l.contains("mytool") && l.contains("pass")));
    assert!(s.lines().any(|l| l.contains("mytool") && l.contains("pass")), "{s}");

    // r: the Connect flow again, on the same program
    keys(&mut op, vec![press(KeyCode::Char('r'))]);
    wait("the flow again", || queued(&world, "flow:connect").iter().any(|g| *g == agent.display().to_string()));
    // it asks leave to look at the program: not given, and the flow ends
    op.until("the leave to look", |s| s.contains("connect-probe") && s.contains("y yes"));
    keys(&mut op, vec![alt('n')]);
    // the flow is over and no card stands over the tab, or the next key would be an answer
    op.until("no question left", |s| !s.contains("y yes · n no"));
    wait("the flow to end", || world.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()));
    op.until("no question left", |s| !s.contains("y yes · n no"));

    // o: behind the confirm; nothing is written until it is answered
    // the operator's folder of this project holds the row, as the Connect flow writes it, and the switch goes in the same file
    let off = here_dir(&d.j4n_home(), &d.project()).join("connectors/mytool.toml");
    assert!(!fs::read_to_string(&off).unwrap().contains("off"));
    keys(&mut op, vec![press(KeyCode::Char('o'))]);
    let s = op.until("the confirm", |s| s.contains("y yes · n no"));
    assert!(s.contains("mytool"), "the card names the connection: {s}");
    assert!(!fs::read_to_string(&off).unwrap().contains("off"), "asked first");
    // the confirm of a key of a row is the card's own: `y` answers it
    keys(&mut op, vec![press(KeyCode::Char('y'))]);
    wait("the edit", || fs::read_to_string(&off).unwrap().contains("off = true"));
    assert!(world.front.knows_doer("mytool"), "it is read at the start: the doer is still here until the next one");
    let s = op.until("the row off", |s| s.lines().any(|l| l.contains("mytool") && l.contains("off")));
    assert!(s.contains("next start") || s.lines().any(|l| l.contains("mytool") && l.contains(" off ")), "{s}");
    world.close();
    drop(op);
    drop(world);

    // the next start: the connection is not a doer, and its row says off
    let world = open(&d);
    assert!(!world.front.knows_doer("mytool"), "off leaves it out of the doers");
    let mut op = Operator::sit(&world);
    op.type_line(":connections");
    op.until("off", |s| s.lines().any(|l| l.contains("mytool") && l.contains(" off ")));
    world.close();
}

/// A run of `id` that said `lines` lines, `read` of them understood.
fn health(world: &World, id: &str, program: &Path, read: u64, lines: u64) {
    let data = serde_json::json!({ "program": program.display().to_string(), "lines": lines, "read": read, "unread": lines - read, "ended": read > 0, "crashed": false, "shape": "0", "drifted": read == 0 });
    world.front.log().append(j4n_core::log::New::of("run.health", j4n_core::change::Actor::Doer(id.into()), data)).expect("written");
}

#[test]
fn a_run_that_said_nothing_the_map_could_read_marks_its_row_and_learns_the_program_again_until_a_good_run_clears_it() {
    let d = Dirs::new("connect-drift");
    let (agent, _) = furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":connections");
    op.until("the rows", |s| s.contains("mytool") && s.lines().any(|l| l.contains("mytool") && l.contains(" ok ")));
    // a run in which every line was read starts nothing
    health(&world, "mytool", &agent, 5, 5);
    // one in which none was: the Connect flow runs for the program, and the row says it learns
    health(&world, "mytool", &agent, 0, 3);
    wait("the flow for the program", || queued(&world, "flow:connect") == [agent.display().to_string()]);
    op.until("learning", |s| s.lines().any(|l| l.contains("mytool") && l.contains("learning")));
    // the flow asks leave to look at the program; not given, so it ends and the row is still the one that drifted
    // the card names the program by its path, so where its words wrap depends on how long the path is, and it stands over the rows: it is
    // known by a word that never wraps
    op.until("the leave to look", |s| s.contains("(`--version`,"));
    keys(&mut op, vec![alt('n')]);
    wait("the flow to end", || world.front.tasks().book().rows().iter().all(|r| r.state.is_terminal()));
    op.until("drifted", |s| s.lines().any(|l| l.contains("mytool") && l.contains("drifted")));
    assert_eq!(queued(&world, "flow:connect").len(), 1, "the run that was read started nothing");
    // the next run that is read clears it
    health(&world, "mytool", &agent, 4, 4);
    op.until("ok", |s| s.lines().any(|l| l.contains("mytool") && l.contains(" ok ")));
    world.close();
}

#[test]
fn with_connect_relearn_off_a_drifted_row_says_so_and_nothing_is_started() {
    let d = Dirs::new("connect-drift-off");
    let (agent, _) = furnish(&d);
    let here = here_dir(&d.j4n_home(), &d.project());
    put(&here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n[[setting]]\nid = \"connect.relearn\"\nvalue = false\n");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":connections");
    op.until("the rows", |s| s.lines().any(|l| l.contains("mytool") && l.contains(" ok ")));
    health(&world, "mytool", &agent, 0, 3);
    op.until("drifted", |s| s.lines().any(|l| l.contains("mytool") && l.contains("drifted")));
    std::thread::sleep(Duration::from_millis(500));
    assert!(queued(&world, "flow:connect").is_empty(), "the row is left as it is: {:?}", queued(&world, "flow:connect"));
    world.close();
}
