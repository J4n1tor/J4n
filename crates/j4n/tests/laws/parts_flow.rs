//! The part runner through the binary's own wiring and the scripted `test` doer, on a
//! fixture repository made with `git init` in the test's own temporary project folder (its top level is checked to be that folder before
//! anything runs, and the doer refuses any other, so no test can commit to J4n's own repository). A document's parts are a Draft until
//! `:parts go`; each part's turn is a plan item, its gate runs after it, a green gate is committed with the paths the turn changed only, a
//! red one stops the run; `parts.max` bounds a run; with the switch off nothing of it happens.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::front::Said;
use j4n_core::gate::check::{DEFAULT, Gates, check};
use j4n_core::gate::{Action, NoGrants, Verdict};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::Mode;
use j4n_parts::doc;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The fixture repository: the project folder is the top of its own work tree, with one commit and a file the operator left changed.
fn repo(d: &Dirs) -> String {
    let dir = d.project();
    git(&dir, &["init", "-q", "-b", "trunk"]);
    for (k, v) in [("user.name", "Fixture"), ("user.email", "fixture@example.invalid"), ("commit.gpgsign", "false")] {
        git(&dir, &["config", k, v]);
    }
    let top = git(&dir, &["rev-parse", "--show-toplevel"]);
    assert_eq!(Path::new(&top).canonicalize().unwrap(), dir.canonicalize().unwrap(), "the fixture is its own repository, never J4n's");
    d.put("notes.txt", "the operator's\n");
    d.put(".gitignore", ".j4n/\n");
    git(&dir, &["add", "notes.txt", ".gitignore"]);
    git(&dir, &["commit", "-q", "-m", "start"]);
    d.put("notes.txt", "the operator's, changed and not committed\n");
    git(&dir, &["rev-parse", "HEAD"])
}

/// A world with the part runner's settings, a fixture repository and its roadmap document.
fn rig(tag: &str, settings: &[(&str, &str)], roadmap: &str, brief: bool) -> (Dirs, World, String) {
    let d = Dirs::new(tag);
    let start = repo(&d);
    d.put("ROADMAP.md", roadmap);
    if brief {
        fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
        fs::write(d.j4n_home().join("connectors/test.toml"), "api = 1\n[[connector]]\nid = \"test\"\nis = \"agent\"\ndialect = \"test\"\nroles = [\"scripted\"]\nbrief = true\n").unwrap();
    }
    // the ledger is not what these tests count
    let mut rows = String::from("api = 1\n[[setting]]\nid = \"ledger.mode\"\nvalue = \"off\"\n");
    for (id, value) in settings {
        rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(d.j4n_home().join("settings/parts.toml"), rows).unwrap();
    let w = open(&d);
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    (d, w, start)
}

fn dump(w: &World) -> String {
    w.front.log().sync();
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn notes(w: &World) -> Vec<String> {
    events(w, "chat.note").iter().map(|n| n["text"].as_str().unwrap_or_default().to_string()).collect()
}

/// The state of the plan item `id`, as the Log last said it.
fn state(w: &World, id: &str) -> String {
    events(w, "plan.item").iter().rfind(|e| e["id"] == id && e["op"] == "state").map(|e| e["state"].as_str().unwrap_or_default().to_string()).unwrap_or_else(|| "todo".into())
}

/// The screen's boundary, over and over (it calls `plan_step` when a task of the thread ends), until `ready`.
fn drive(w: &World, what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ready() {
        assert!(Instant::now() < end, "never: {what}\n{}", dump(w));
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// The commits since `start`, oldest first: (subject, the files it holds).
fn commits(d: &Dirs, start: &str) -> Vec<(String, Vec<String>)> {
    let dir = d.project();
    let list = git(&dir, &["rev-list", "--reverse", &format!("{start}..HEAD")]);
    list.lines()
        .filter(|l| !l.is_empty())
        .map(|sha| {
            let subject = git(&dir, &["log", "-1", "--format=%s", sha]);
            let files = git(&dir, &["show", "--name-only", "--format=", sha]).lines().map(String::from).collect();
            (subject, files)
        })
        .collect()
}

fn say(w: &World, line: &str) -> Said {
    w.front.say("main", line)
}

const THREE: &str = "# Roadmap\n\nSome words first.\n\n| Part | Content | Gate |\n|---|---|---|\n| **A1** | write a.txt alpha | `test -f a.txt` |\n| **A2** | write b.txt beta | `test -f b.txt` · S161 |\n| **A3** | write c.txt gamma | `test -f c.txt` |\n";

#[test]
fn under_the_default_single_go_runs_its_parts_and_a_plan_of_the_operators_own_is_not_stepped() {
    let (d, w, start) = rig("parts-under-default-single", &[("parts.mode", "\"auto\"")], THREE, false);
    assert_eq!(w.front.lanes_now().word(), "single", "the default word");
    assert!(matches!(say(&w, ":parts ROADMAP.md"), Said::Command(_)), "{}", dump(&w));
    assert!(matches!(say(&w, ":parts go"), Said::Command(_)), "{}", dump(&w));
    drive(&w, "three parts merged", || ["a1", "a2", "a3"].iter().all(|i| state(&w, i) == "merged"));
    assert_eq!(commits(&d, &start).len(), 3, "{}", dump(&w));
    assert!(notes(&w).iter().any(|n| n.starts_with("plan done: 3/3 done")), "{:?}", notes(&w));
    // the run took no branch and made no plan of its own: `single` is the word still
    assert!(events(&w, "plan.routed").iter().all(|r| r["place"] == "inline"), "{}", dump(&w));
    assert!(events(&w, "plan.asked").is_empty(), "{}", dump(&w));
    w.close();
    // a plan the operator adds by hand (no run of parts) is not stepped under `single`: its lines drive it
    let (d, w, _) = rig("parts-own", &[("parts.mode", "\"auto\"")], THREE, false);
    let Said::Started { task } = say(&w, "say once") else { panic!("{}", dump(&w)) };
    w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
    w.front.log().append(j4n_core::log::New::of("plan.item", Actor::Operator, serde_json::json!({ "op": "add", "id": "own", "goal": "write own.txt" })).thread("main")).unwrap();
    for _ in 0..10 {
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(state(&w, "own"), "todo", "{}", dump(&w));
    assert!(!d.project().join("own.txt").exists());
    w.close();
}

#[test]
fn three_parts_are_three_turns_three_green_gates_and_three_real_commits_of_the_changed_paths_only() {
    let (d, w, start) = rig("parts-three-are-three", &[("parts.mode", "\"auto\"")], THREE, false);
    assert!(matches!(say(&w, ":parts ROADMAP.md"), Said::Command(_)), "{}", dump(&w));
    let card = notes(&w).last().cloned().unwrap();
    assert!(card.starts_with("◆ parts · ROADMAP.md · 3 parts") && card.contains("│ ▸ 1 a1 · write a.txt alpha ⟂ gate `test -f a.txt`") && card.contains(":parts go"), "{card}");
    assert!(events(&w, "plan.item").is_empty(), "nothing is added before `go`\n{}", dump(&w));
    assert!(matches!(say(&w, ":parts go"), Said::Command(_)), "{}", dump(&w));
    drive(&w, "three parts merged", || ["a1", "a2", "a3"].iter().all(|i| state(&w, i) == "merged"));
    let got = commits(&d, &start);
    assert_eq!(
        got,
        vec![
            ("J4n part a1: write a.txt alpha".to_string(), vec!["a.txt".to_string()]),
            ("J4n part a2: write b.txt beta".to_string(), vec!["b.txt".to_string()]),
            ("J4n part a3: write c.txt gamma".to_string(), vec!["c.txt".to_string()]),
        ],
        "{}",
        dump(&w)
    );
    // the operator's own change is in none of them, and the branch is the one checked out
    let dir = d.project();
    assert_eq!(git(&dir, &["status", "--porcelain", "--", "notes.txt"]), "M notes.txt");
    assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), "trunk");
    let body = git(&dir, &["log", "-1", "--format=%b"]);
    assert!(body.starts_with("gate: `test -f c.txt` — green (r1, part 3 of 3, ROADMAP.md)"), "{body}");
    // each part was its own turn of the main lane, in plan order, each after its gate and its commit
    let steps: Vec<String> = events(&w, "parts.step").iter().map(|s| format!("{}:{}", s["part"].as_str().unwrap(), s["phase"].as_str().unwrap())).collect();
    assert_eq!(steps, ["a1:gate", "a1:commit", "a1:done", "a2:gate", "a2:commit", "a2:done", "a3:gate", "a3:commit", "a3:done"], "{}", dump(&w));
    let book = w.front.tasks().book();
    let turns: Vec<_> = book.rows().into_iter().filter(|r| r.spec.doer == "test").collect();
    assert_eq!(turns.len(), 3, "{}", dump(&w));
    // `compact` (the default) is the boundary of the compaction: the next part is the next turn of the same session
    assert_eq!(turns[1].spec.follows.as_deref(), Some(turns[0].id.as_str()));
    assert_eq!(turns[2].spec.follows.as_deref(), Some(turns[1].id.as_str()));
    let kids: Vec<_> = book.rows().into_iter().filter(|r| r.spec.doer == "parts").collect();
    assert_eq!(kids.len(), 6, "a gate and a commit for each part");
    assert!(kids.iter().all(|k| k.spec.role.as_deref() == Some("parts") && k.spec.thread.as_deref() == Some("main")));
    assert!(notes(&w).iter().any(|n| n.starts_with("plan done: 3/3 done")), "the plan keeper's report");
    w.close();
}

#[test]
fn a_red_gate_stops_the_run_and_commits_nothing() {
    let roadmap = THREE.replace("`test -f b.txt` · S161", "`test -f nothere.txt`");
    let (d, w, start) = rig("parts-red", &[("parts.mode", "\"auto\"")], &roadmap, false);
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "a2 blocked", || state(&w, "a2") == "blocked");
    // let the boundary come round a few more times: nothing else starts
    for _ in 0..10 {
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(state(&w, "a1"), "merged");
    assert_eq!(state(&w, "a3"), "todo", "{}", dump(&w));
    let got = commits(&d, &start);
    assert_eq!(got.len(), 1, "a1's commit and none of a2's: {got:?}");
    assert!(d.project().join("b.txt").exists(), "a2's turn wrote its file");
    assert_eq!(git(&d.project(), &["status", "--porcelain", "--", "b.txt"]), "?? b.txt", "and it was not committed");
    assert!(notes(&w).iter().any(|n| n.contains("a2 stopped the run: its gate `test -f nothere.txt` is red") && n.contains("nothing of it was committed")), "{:?}", notes(&w));
    assert_eq!(w.front.tasks().book().rows().into_iter().filter(|r| r.spec.doer == "test").count(), 2, "a3 never started");
    // a stopped part is the operator's to decide: no replan is asked of a planner, and the plan keeper's report says so
    assert!(events(&w, "plan.asked").is_empty(), "{}", dump(&w));
    assert!(notes(&w).iter().any(|n| n.starts_with("plan blocked: 1/3 done (a1)") && n.contains("yours to decide — a2: its gate `test -f nothere.txt` is red")), "{:?}", notes(&w));
    w.close();
}

#[test]
fn parts_max_two_stops_after_two_and_the_next_go_takes_the_third() {
    let (d, w, start) = rig("parts-max", &[("parts.mode", "\"auto\""), ("parts.max", "2")], THREE, false);
    say(&w, ":parts ROADMAP.md");
    assert!(notes(&w).last().unwrap().contains("the next run takes 2 (parts.max 2)"));
    say(&w, ":parts go");
    drive(&w, "two merged", || state(&w, "a1") == "merged" && state(&w, "a2") == "merged");
    for _ in 0..10 {
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(commits(&d, &start).len(), 2);
    assert!(!events(&w, "plan.item").iter().any(|e| e["id"] == "a3"), "the third is not in the plan\n{}", dump(&w));
    assert!(matches!(say(&w, ":parts go"), Said::Command(_)), "{}", dump(&w));
    drive(&w, "a3 merged", || state(&w, "a3") == "merged");
    assert_eq!(commits(&d, &start).len(), 3);
    assert!(matches!(say(&w, ":parts go"), Said::Refused(why) if why.contains("every part of ROADMAP.md has run")));
    w.close();
}

#[test]
fn show_waits_after_a_green_gate_and_commits_on_next() {
    let (d, w, start) = rig("parts-show", &[("parts.mode", "\"show\""), ("parts.max", "1")], THREE, false);
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "a1 ready", || events(&w, "parts.step").iter().any(|s| s["phase"] == "ready"));
    for _ in 0..10 {
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(commits(&d, &start).is_empty(), "show commits nothing by itself");
    assert_eq!(state(&w, "a1"), "running");
    assert!(notes(&w).iter().any(|n| n == "◆ part a1 · gate `test -f a.txt` green · `:parts next` commits it and goes on"), "{:?}", notes(&w));
    assert!(matches!(say(&w, ":parts next"), Said::Command(_)), "{}", dump(&w));
    drive(&w, "a1 merged", || state(&w, "a1") == "merged");
    assert_eq!(commits(&d, &start).len(), 1);
    assert!(matches!(say(&w, ":parts next"), Said::Refused(_)));
    w.close();
}

#[test]
fn reset_starts_each_part_in_a_new_session_with_the_parts_brief_and_the_must_keep() {
    let (_d, w, _) = rig("parts-reset", &[("parts.mode", "\"auto\""), ("parts.boundary", "\"reset\""), ("parts.max", "2")], THREE, true);
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "two merged", || state(&w, "a1") == "merged" && state(&w, "a2") == "merged");
    let turns: Vec<_> = w.front.tasks().book().rows().into_iter().filter(|r| r.spec.doer == "test").collect();
    assert_eq!(turns.len(), 2);
    assert!(turns.iter().all(|t| t.spec.follows.is_none() && t.spec.resume.is_none()), "a new session each part");
    let briefs: Vec<String> = turns.iter().map(|t| t.spec.input["brief"].as_str().unwrap_or_default().to_string()).collect();
    assert!(briefs[0].contains("## parts\ndocument: ROADMAP.md (run r1, part 1 of 2)\nnow: a1 — write a.txt alpha"), "{}", briefs[0]);
    assert!(!briefs[0].contains("MUST KEEP"), "the first part has nothing to keep yet");
    assert!(briefs[1].contains("now: a2 — write b.txt beta") && briefs[1].contains("done: a1 — write a.txt alpha · gate green, committed "), "{}", briefs[1]);
    assert!(briefs[1].contains("MUST KEEP (re-stated after compaction):\n- goal: the parts of ROADMAP.md, one at a time; this one is a2\n- decisions:\n  · write a.txt? → yes\n- changed files: a.txt\n"), "{}", briefs[1]);
    w.close();
}

#[test]
fn the_draft_is_the_operators_keep_and_drop_change_what_runs_and_a_plan_that_is_open_refuses_go() {
    let (d, w, start) = rig("parts-draft", &[("parts.mode", "\"auto\"")], THREE, false);
    say(&w, ":parts ROADMAP.md");
    assert!(matches!(say(&w, ":parts keep 3 1"), Said::Command(_)));
    let card = notes(&w).last().cloned().unwrap();
    assert!(card.contains("2 parts") && card.contains("1 a3 · ") && card.contains("2 a1 · ") && !card.contains("a2"), "{card}");
    assert!(matches!(say(&w, ":parts drop 2"), Said::Command(_)));
    assert!(matches!(say(&w, ":parts keep 9"), Said::Refused(_)));
    assert!(events(&w, "plan.item").is_empty());
    say(&w, ":parts go");
    drive(&w, "a3 merged", || state(&w, "a3") == "merged");
    assert_eq!(commits(&d, &start).iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(), ["J4n part a3: write c.txt gamma"]);
    // the operator's own plan item, open: a run waits for it
    w.front.log().append(j4n_core::log::New::of("plan.item", Actor::Operator, serde_json::json!({ "op": "add", "id": "mine", "goal": "hang" })).thread("main")).unwrap();
    assert!(matches!(say(&w, ":parts ROADMAP.md"), Said::Command(_)));
    assert!(matches!(say(&w, ":parts go"), Said::Refused(why) if why.contains("the plan is open (mine (")), "{}", dump(&w));
    w.close();
}

// ---- Tier 0: the switch off ----

#[test]
fn the_switch_off_writes_nothing_and_a_plan_runs_as_it_did() {
    for (tag, settings, pack_off) in [("parts-switch-off-writes", vec![], false), ("parts-switch-off-writes-2", vec![("parts.mode", "\"maybe\"")], false), ("parts-switch-off-writes-3", vec![("parts.mode", "\"auto\"")], true)] {
        let d = Dirs::new(tag);
        let start = repo(&d);
        d.put("ROADMAP.md", THREE);
        let mut rows = String::from("api = 1\n");
        for (id, value) in &settings {
            rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
        }
        if !pack_off {
            fs::write(d.j4n_home().join("settings/parts.toml"), rows).unwrap();
        } else {
            d.pack_off("parts");
        }
        // the plan here is the operator's own, not a run of parts: the keeper steps it under `auto` only
        fs::write(d.j4n_home().join("settings/lanes.toml"), "api = 1\n[[setting]]\nid = \"lanes\"\nvalue = \"auto\"\n").unwrap();
        let w = open(&d);
        assert!(w.notices.is_empty(), "{tag}: {:?}", w.notices);
        assert!(matches!(say(&w, ":parts ROADMAP.md"), Said::Refused(_)), "{tag}");
        let Said::Started { task } = say(&w, "say bound") else { panic!("{tag}: the first line binds the thread") };
        w.front.tasks().book().wait(&task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap();
        // a plan item of the operator's runs inline and is merged when its turn ends well, as before
        w.front.log().append(j4n_core::log::New::of("plan.item", Actor::Operator, serde_json::json!({ "op": "add", "id": "a1", "goal": "write a.txt alpha" })).thread("main")).unwrap();
        w.front.plan_step("main");
        drive(&w, "a1 merged", || state(&w, "a1") == "merged");
        w.front.log().sync();
        assert_eq!(w.front.log().read(0).filter(|e| e.kind.starts_with("parts.")).count(), 0, "{tag}\n{}", dump(&w));
        assert!(commits(&d, &start).is_empty(), "{tag}: nothing commits");
        assert!(!w.front.tasks().book().rows().iter().any(|r| r.spec.doer == "parts"), "{tag}");
        w.close();
    }
}

// ---- the Gate ----

fn gates() -> Gates {
    let parts = Layer::new(LayerKind::Pack, "parts", j4n::pack::PARTS.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect());
    let (tables, problems) = Tables::load(vec![j4n::pack::base(), parts]);
    assert!(problems.is_empty(), "{problems:?}");
    let (gates, found) = Gates::load(&tables);
    assert!(found.is_empty(), "{found:?}");
    gates
}

fn verdict(g: &Gates, actor: &str, line: &str) -> Verdict {
    check(g, &NoGrants, &Action::shell(Actor::Doer(actor.into()), line))
}

#[test]
fn push_is_refused_whoever_asks_and_the_pack_allows_only_its_own_add_and_commit() {
    let g = gates();
    for push in ["git push", "git push origin trunk", "git push --force origin trunk", "git -C . push", "git add -- a.txt && git push"] {
        assert!(matches!(verdict(&g, "parts", push), Verdict::Block { ref by, .. } if by == "push-is-yours"), "{push}: {:?}", verdict(&g, "parts", push));
    }
    let row = |v: Verdict| matches!(v, Verdict::Allow { ref by } if by == "parts-commits-its-part");
    let commit = j4n_parts::git::line(&["git", "commit", "-m", "J4n part a1: write a.txt alpha", "-m", "gate: `test -f a.txt` — green", "-m", "> it's done", "--", "a.txt"].map(String::from));
    assert!(row(verdict(&g, "parts", "git add -- a.txt b/c.txt")), "{:?}", verdict(&g, "parts", "git add -- a.txt"));
    assert!(row(verdict(&g, "parts", &commit)), "{commit}: {:?}", verdict(&g, "parts", &commit));
    // not the row: another doer, everything, a compound line, a line that is no add or commit
    for (actor, line) in [("test", "git add -- a.txt"), ("parts", "git add -A"), ("parts", "git add ."), ("parts", "git add -- a.txt; rm -rf x"), ("parts", "git commit -am x"), ("parts", "git reset --hard")] {
        assert!(!row(verdict(&g, actor, line)), "{actor}: {line}: {:?}", verdict(&g, actor, line));
    }
    assert!(matches!(verdict(&g, "parts", "git add -A"), Verdict::Allow { ref by } if by == DEFAULT), "nobody's yes: the operator is asked");
    // the danger matcher's own block still wins: writing into .git
    assert!(matches!(verdict(&g, "parts", "git add -- x > .git/hooks/pre-commit"), Verdict::Block { .. }));
}

#[test]
fn the_doer_commits_only_in_a_folder_that_is_the_top_of_its_own_repository() {
    // a folder inside a repository, and a folder with none: the commit is refused and the outer repository is untouched
    let d = Dirs::new("parts-top");
    let start = repo(&d);
    d.put("inner/ROADMAP.md", THREE);
    d.put("inner/x.txt", "x");
    let setup = Setup { root: d.project().join("inner"), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Full), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    fs::write(d.j4n_home().join("settings/parts.toml"), "api = 1\n[[setting]]\nid = \"parts.mode\"\nvalue = \"auto\"\n").unwrap();
    let w = wire::open(setup, &opts, true, "main").unwrap();
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "a1 blocked", || state(&w, "a1") == "blocked");
    assert!(notes(&w).iter().any(|n| n.contains("its commit failed: no commit: the folder is not the top of its own git work tree")), "{:?}", notes(&w));
    assert!(commits(&d, &start).is_empty());
    w.close();
    let e = Dirs::new("parts-none");
    e.put("ROADMAP.md", THREE);
    assert!(Command::new("git").args(["rev-parse", "--show-toplevel"]).current_dir(e.project()).output().is_ok_and(|o| !o.status.success() || Path::new(String::from_utf8_lossy(&o.stdout).trim()) != e.project()), "the temporary folder is in no repository of its own");
    fs::write(e.j4n_home().join("settings/parts.toml"), "api = 1\n[[setting]]\nid = \"parts.mode\"\nvalue = \"auto\"\n").unwrap();
    let w = open(&e);
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "a1 blocked", || state(&w, "a1") == "blocked");
    assert!(notes(&w).iter().any(|n| n.contains("its commit failed: no commit:")), "{:?}", notes(&w));
    w.close();
}

#[test]
fn a_document_outside_the_project_is_refused() {
    let (d, w, _) = rig("parts-out", &[("parts.mode", "\"auto\"")], THREE, false);
    fs::write(d.dir.join("outside.md"), THREE).unwrap();
    assert!(matches!(say(&w, ":parts ../outside.md"), Said::Refused(why) if why.contains("outside the project folder")));
    assert!(matches!(say(&w, ":parts nothing.md"), Said::Refused(_)));
    w.close();
}

// ---- the document reader ----

#[test]
fn the_reader_takes_the_gated_table_else_the_steps_else_the_first_table() {
    let ids = |t: &str| doc::read(t).iter().map(|p| format!("{}={}|{}|{}", p.id, p.words, p.gate, p.cmd.join(" "))).collect::<Vec<_>>();
    // a table with a Gate column wins over an earlier table and over steps
    let t = "| a | b |\n|---|---|\n| x | y |\n\n### S1 step\n\n| Step | Content | On | Gate test |\n|:--|--|--|--|\n| **A1** | pack `parts` | X1 | S1 · E-x |\n| | the rest | | `cargo test -p j4n --test laws parts` |\n| A1 | twice | | `true` |\n";
    assert_eq!(ids(t), ["a1=pack parts|S1 · E-x|", "p2=the rest|cargo test -p j4n --test laws parts|cargo test -p j4n --test laws parts", "p3=twice|true|true"]);
    // steps: the heading's first word is the id, the text under it the words, a `Gate:` line the gate; a code fence is not a heading
    let s = "# Plan\n\n### W1 the first\nDo this.\n```\n### not a step\n```\nGate: `test -f a`\n\n### the second\nAnd that.\n## Other\nnot part of it\n";
    assert_eq!(ids(s), ["w1=W1 the first\nDo this.\n```\n### not a step\n```|test -f a|test -f a", "the=the second\nAnd that.||"]);
    // no gate column and no steps: the first table, its second column the words
    assert_eq!(ids("| Id | What |\n|---|---|\n| one | a |\n| two | b |\n"), ["one=a||", "two=b||"]);
    // a `|` inside backticks is the cell's
    assert_eq!(ids("| P | Content | Gate |\n|---|---|---|\n| p | x | `a | b` |\n"), ["p=x|a | b|a | b"]);
    assert!(doc::read("just words\n").is_empty());
}

#[test]
fn a_part_waits_for_its_gate_and_one_with_no_command_needs_the_projects_check_to_pass() {
    // a slow gate: while it runs the part stays `running` and nothing is committed
    let roadmap = "| Part | Content | Gate |\n|---|---|---|\n| s1 | write s.txt slow | `sleep 1` |\n| n1 | write n.txt none | S161, no command |\n";
    let (d, w, start) = rig("parts-wait", &[("parts.mode", "\"auto\"")], roadmap, false);
    say(&w, ":parts ROADMAP.md");
    say(&w, ":parts go");
    drive(&w, "s1's gate runs", || events(&w, "parts.step").iter().any(|s| s["part"] == "s1" && s["phase"] == "gate"));
    for _ in 0..5 {
        w.front.plan_step("main");
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(state(&w, "s1"), "running", "{}", dump(&w));
    assert!(commits(&d, &start).is_empty());
    drive(&w, "s1 merged", || state(&w, "s1") == "merged");
    assert_eq!(commits(&d, &start).len(), 1);
    // no command: the project's check, and no check applies in the fixture: that is no proof, so no commit
    drive(&w, "n1 blocked", || state(&w, "n1") == "blocked");
    assert_eq!(commits(&d, &start).len(), 1);
    assert!(notes(&w).iter().any(|n| n.contains("n1 stopped the run: its gate the project's check is red: no check applies")), "{:?}", notes(&w));
    w.close();
}
