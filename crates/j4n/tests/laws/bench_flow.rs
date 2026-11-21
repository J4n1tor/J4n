//! The bench pack: a case is data and a run of it is a doer — a fresh folder, the lines said to the doer under test in turn,
//! the answers held to what the case expects, the folder checked at the end, and the result written down. The offline arm: the doer
//! under test is a script that does what the cases ask.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::gate::grant::here_dir;
use j4n_core::task::Mode;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn say(op: &mut Operator, world: &World, line: &str, answer: &str) -> String {
    crate::screen_drive::wait_idle(world);
    op.type_line(line);
    op.until(answer, |s| s.contains(answer))
}

/// A doer that does what the two cases ask, in the folder it is started in, and one that does nothing.
fn furnish(d: &Dirs) {
    let bin = d.dir.join("bin");
    let worker = bin.join("worker");
    fs::write(
        &worker,
        "#!/bin/sh\ncase \"$1\" in\n  *Append*) echo two >> notes.txt; echo \"done: appended two\" ;;\n  *check.sh*) echo port=8080 > config.txt; echo \"changed config.txt\" ;;\n  *port*) echo \"It is 8080 now.\" ;;\n  *) echo \"hello\" ;;\nesac\n",
    )
    .unwrap();
    let idler = bin.join("idler");
    fs::write(&idler, "#!/bin/sh\necho \"I looked at it.\"\n").unwrap();
    for p in [&worker, &idler] {
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = fs::create_dir_all(d.j4n_home());
    fs::write(d.j4n_home().join("harness.toml"), "api = 1\n[[enable]]\nid = \"bench\"\n").unwrap();
    let here = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(here.join("connectors")).unwrap();
    fs::create_dir_all(here.join("cases")).unwrap();
    fs::write(here.join("settings.toml"), "api = 1\n[[setting]]\nid = \"box.unavailable\"\nvalue = \"here\"\n").unwrap();
    for (id, p) in [("worker", &worker), ("idler", &idler)] {
        fs::write(here.join(format!("connectors/{id}.toml")), format!("api = 1\n[[connector]]\nid = \"{id}\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"{}\"\nstart = [\"{{prompt}}\"]\n", p.display())).unwrap();
    }
    fs::write(
        here.join("cases/scripted.toml"),
        "api = 1\n[[case]]\nid = \"writes\"\ncheck = \"test -f out.txt\"\n[[case.turns]]\nsay = \"write out.txt hi; say wrote\"\n\n[[case]]\nid = \"crashes\"\n[[case.turns]]\nsay = \"fail boom\"\n\n[[case]]\nid = \"carries-on\"\n[[case.turns]]\nsay = \"who\"\n[[case.turns]]\nsay = \"who\"\nexpect = \"contains(answer, 'turn 2')\"\n",
    )
    .unwrap();
    fs::write(here.join("cases/mine.toml"), "api = 1\n[[case]]\nid = \"once\"\nholdout = true\nfiles = { \"a.txt\" = \"x\\n\" }\ncheck = \"test -f a.txt\"\n[[case.turns]]\nsay = \"hello\"\nexpect = \"contains(answer, 'hello')\"\n").unwrap();
}

#[test]
fn a_case_is_run_in_a_fresh_folder_with_the_doer_under_test_and_what_came_of_it_is_written_down() {
    let d = Dirs::new("bench-run");
    furnish(&d);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    let s = say(&mut op, &world, ":bench note-appended doer=worker", "note-appended with worker: pass");
    assert!(s.contains("1 of 1 lines right") && s.contains("folder as asked"), "{s}");
    let s = say(&mut op, &world, ":bench port-fixed doer=worker", "port-fixed with worker: pass");
    assert!(s.contains("2 of 2 lines right"), "{s}");
    // a doer that only looks fails both the answer and the folder
    let s = say(&mut op, &world, ":bench note-appended doer=idler", "note-appended with idler: FAIL");
    assert!(s.contains("0 of 1 lines right") && s.contains("folder not as asked"), "{s}");
    drop(op);

    world.front.log().sync();
    let runs: Vec<_> = world.front.log().read(0).filter(|e| e.kind == "bench.case").collect();
    assert_eq!(runs.len(), 3);
    let first = &runs[0].data;
    assert_eq!((first["case"].as_str(), first["doer"].as_str(), first["pass"].as_bool()), (Some("note-appended"), Some("worker"), Some(true)));
    // the case ran in a folder of its own, made from the case, and left there to be looked at
    let folder = std::path::PathBuf::from(first["folder"].as_str().unwrap());
    assert!(folder.starts_with(d.j4n_home().join("bench")), "{folder:?}");
    assert_eq!(fs::read_to_string(folder.join("notes.txt")).unwrap(), "one\ntwo\n");
    assert!(!d.project().join("notes.txt").exists(), "nothing was written into the project");
    // the case with `git` started as a repository
    let second = std::path::PathBuf::from(runs[1].data["folder"].as_str().unwrap());
    assert!(second.join(".git").is_dir());
    assert_eq!(fs::read_to_string(second.join("config.txt")).unwrap().trim(), "port=8080");
    drop(world);
}

#[test]
fn a_hold_out_is_scored_once_for_a_doer_and_a_run_that_cannot_be_made_says_why() {
    let d = Dirs::new("bench-once");
    furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":bench once doer=worker", "once with worker: pass");
    let s = say(&mut op, &world, ":bench once doer=worker", "scored once for worker already");
    assert!(s.contains("again runs it anyway"), "{s}");
    say(&mut op, &world, ":bench once doer=idler", "once with idler: FAIL");
    say(&mut op, &world, ":bench once doer=worker again", "once with worker: pass");
    // `all` leaves a hold-out alone
    let s = say(&mut op, &world, ":bench all doer=worker", "port-fixed with worker: pass");
    assert!(!s.lines().last().unwrap_or_default().contains("once"), "{s}");
    say(&mut op, &world, ":bench nothing doer=worker", "no case called `nothing`");
    say(&mut op, &world, ":bench once doer=nobody", "no doer called `nobody`");
    say(&mut op, &world, ":bench once", "name the doer to run it with");
    world.front.log().sync();
    let held = world.front.log().read(0).filter(|e| e.kind == "bench.case" && e.data["case"] == "once").count();
    assert_eq!(held, 3, "worker, idler, and worker again");
    drop(op);
    drop(world);
}

#[test]
fn the_results_page_shows_the_last_run_of_each_case_for_each_doer() {
    let d = Dirs::new("bench-page");
    furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    say(&mut op, &world, ":bench note-appended doer=worker", "note-appended with worker: pass");
    say(&mut op, &world, ":bench note-appended doer=idler", "note-appended with idler: FAIL");
    crate::screen_drive::wait_idle(&world);
    op.type_line(":results");
    let row = |s: &str, id: &str, rest: &str| s.lines().any(|l| l.contains(id) && l.contains(rest) && !crate::screen_drive::is_you(l));
    let s = op.until("the page", |s| row(s, "note-appended/worker", " on ") && row(s, "note-appended/idler", " off "));
    assert!(s.contains("Bench"), "{s}");
    drop(op);
    drop(world);
}

#[test]
fn a_doer_is_asked_nothing_it_may_not_have_unless_the_run_is_full_and_a_failed_line_is_a_failed_case() {
    let d = Dirs::new("bench-script");
    furnish(&d);
    let world = open(&d);
    let mut op = Operator::sit(&world);
    // its question about writing is answered no, and the folder is then not as asked
    say(&mut op, &world, ":bench writes doer=test", "writes with test: FAIL");
    say(&mut op, &world, ":bench writes doer=test full", "writes with test: pass");
    // a line that fails is a failed case even where the case expects nothing of the answer
    say(&mut op, &world, ":bench crashes doer=test", "crashes with test: FAIL");
    // the doer carries on in its session from one line to the next: the second line is its second turn in the same child
    say(&mut op, &world, ":bench carries-on doer=test", "carries-on with test: pass");
    crate::screen_drive::wait_idle(&world);
    // `all` runs what is not a hold-out, and the hold-out is left alone
    say(&mut op, &world, ":bench all doer=worker", "port-fixed with worker: pass");
    world.front.log().sync();
    let once = world.front.log().read(0).filter(|e| e.kind == "bench.case" && e.data["case"] == "once").count();
    assert_eq!(once, 0, "a hold-out is not part of `all`");
    drop(op);
    drop(world);
}

#[test]
fn a_case_line_is_given_the_brief_a_run_gives_so_a_pack_is_measured_and_a_bare_case_or_run_leaves_it_out() {
    // the doer under test (a script that takes a Brief, no model) is given the skills of a pack that is on, as a line of `j4n run` is
    let d = Dirs::new("bench-brief");
    furnish(&d);
    let home = d.j4n_home();
    fs::write(home.join("harness.toml"), "api = 1\n[[enable]]\nid = \"bench\"\n").unwrap();
    fs::create_dir_all(home.join("connectors")).unwrap();
    fs::write(home.join("connectors/test.toml"), "api = 1\n[[connector]]\nid = \"test\"\nis = \"agent\"\ndialect = \"test\"\nroles = [\"scripted\"]\nbrief = true\n").unwrap();
    fs::create_dir_all(home.join("skills/measured")).unwrap();
    fs::write(home.join("skills/measured/SKILL.md"), "---\nname: measured\nwhen: widgets\ncost: 30\n---\n# Widget notes\nKeep the widgets flat.\n").unwrap();
    fs::write(
        here_dir(&home, &d.project()).join("cases/brief.toml"),
        "api = 1\n[[case]]\nid = \"with-brief\"\n[[case.turns]]\nsay = \"say fix the widgets\"\n\n[[case]]\nid = \"bare-case\"\nbrief = false\n[[case.turns]]\nsay = \"say fix the widgets\"\n\n[[case]]\nid = \"plain\"\n[[case.turns]]\nsay = \"say fix the widgets\"\n",
    )
    .unwrap();
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    let skills = |w: &World| {
        w.front.log().sync();
        w.front.log().read(0).filter(|e| e.kind == "brief.skills").map(|e| e.data["skills"][0]["id"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>()
    };
    say(&mut op, &world, ":bench with-brief doer=test", "with-brief with test: pass");
    assert_eq!(skills(&world), ["measured"], "the line was given the skill the pack picks for it");
    // a case that says `brief = false`, and a run that says `bare`, bench the doer alone
    say(&mut op, &world, ":bench bare-case doer=test", "bare-case with test: pass");
    say(&mut op, &world, ":bench plain doer=test bare", "plain with test: pass");
    assert_eq!(skills(&world), ["measured"], "no second pick: both runs left the Brief out");
    drop(op);
    world.front.log().sync();
    let briefs: Vec<_> = world.front.log().read(0).filter(|e| e.kind == "bench.case").map(|e| e.data["brief"].as_bool()).collect();
    assert_eq!(briefs, [Some(true), Some(false), Some(false)], "the result says whether the doer had a Brief");
    drop(world);
}
