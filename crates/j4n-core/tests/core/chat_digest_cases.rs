//! What comes back from the chats Main started: a held piece is checked, reviewed on another doer and merged one at a time, a failed check is never merged, a
//! report's claims are looked up in the files, and Main is told by a digest cut to a token cap, never the transcript: a row, a line in the Log, and a section of
//! its next Brief that holds the newest few however many chats there have been. A digest wakes Main only for a decision.

use crate::branch_validate::{A, CLEAN, Held, Rig3, SCREEN_UNLOADED, caps, checker, doer, folder_check};
use crate::front_cases::{Fx, chatty, fx_placed, fx_placed_full};
use crate::scripted::{Beat, Scripted, end};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::digest::{Digest, Parts, line, section};
use serde_json::{Value, json};
use std::sync::Arc;

const ONE: &str = r#"{"decision":"one","chats":[],"features":[],"why":"a question"}"#;
const LINE: &str = "check the service and rewrite its docs";

fn settings(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.orchestrate.ask = "never".into();
    c.orchestrate.doer = "zcheap".into();
}

fn look(_write: bool) -> String {
    // the look sees a part that writes, so the split stands (a split of reads alone stays one lane)
    json!({ "decision": "split", "chats": [{ "goal": "do the part", "writes": true, "doer": "pi", "model": "" }], "features": [], "why": "one part to hand off" }).to_string()
}

fn brief(write: bool) -> Value {
    json!({ "id": "part", "goal": "do the part and report", "paths": A, "writes": if write { vec!["src/a/1.rs"] } else { vec![] }, "done": "it is done", "doer": "pi", "model": "", "mode": if write { "write" } else { "read" }, "features": [], "context": "brief", "checks": [] })
}

fn report(claim_at: Option<&str>) -> String {
    let found: Vec<Value> = claim_at.map(|at| json!({ "claim": "the loop is there", "at": at })).into_iter().collect();
    json!({ "answer": "it is done", "found": found, "changed": "", "ran": [], "sure": "high", "open": [] }).to_string()
}

/// Main answers the planning turn, then works what is left; the look says split; `pi` answers each chat with the next of `reports`.
fn rig(write: bool, reports: &[&str], check: Arc<Scripted>, tune: impl FnOnce(&mut Cfg)) -> Rig3 {
    rig_of(brief(write), write, reports, check, tune)
}

fn rig_of(brief: Value, write: bool, reports: &[&str], check: Arc<Scripted>, tune: impl FnOnce(&mut Cfg)) -> Rig3 {
    rig_with(brief, write, doer("pi", reports), check, tune)
}

/// [`rig_of`] with the chat's doer `pi` as given.
fn rig_with(brief: Value, write: bool, pi: Arc<Scripted>, check: Arc<Scripted>, tune: impl FnOnce(&mut Cfg)) -> Rig3 {
    let held = Held::new();
    let plan = json!({ "chats": [brief], "main": "" }).to_string();
    let doers = vec![doer("claude", &[&plan, "main worked", "main worked", "main worked", "main worked"]), pi, doer("own", &[CLEAN; 6]), check, chatty("zcheap", &[&look(write), ONE])];
    let f: Fx = fx_placed(doers, Some("pi"), vec![held.place()], |c| {
        settings(c);
        tune(c);
    });
    Rig3 { f, held }
}

fn digests(r: &Rig3) -> Vec<Value> {
    r.f.rig.of_kind("orchestrate.digest").into_iter().map(|e| e.data).collect()
}

#[test]
fn a_read_chat_ends_in_a_digest_row_and_a_line_in_the_log_and_main_is_not_woken() {
    let r = rig(false, &[&report(None)], checker(2), |_| {});
    assert!(matches!(r.f.front.say("main", LINE), j4n_core::front::Said::Nothing), "{}", r.f.rig.dump());
    r.until("the scout ended", |r| r.item("c1").done());
    assert_eq!(r.item("c1").state, "reviewed", "a scout has nothing to merge");
    let d = digests(&r);
    assert_eq!(d.len(), 1, "{}", r.f.rig.dump());
    let text = d[0]["text"].as_str().unwrap();
    assert!(text.starts_with("c1 do the part and report · scout reviewed · $0.00 · no claims · sure high · open: none"), "{text}");
    assert!(d[0]["tokens"].as_u64().is_some_and(|t| t <= 400) && d[0]["wake"] == false, "{}", d[0]);
    assert!(r.f.notes().iter().any(|n| n.starts_with("c1 do the part and report · scout reviewed")), "{:?}", r.f.notes());
    assert!(r.f.tasks().iter().all(|t| !t.spec.goal.starts_with("J4n:")), "no turn of Main was started for it");
    // the next turn of Main is shown the chats' states and the digest, in a section of its own
    // every task is over before the next line, or a loaded machine makes that line a steer
    for t in r.f.tasks() {
        r.f.rig.finished(&t.id);
    }
    let said = r.f.front.say("main", "what is the state of things?");
    let brief = r.f.rig.of_kind("brief.rendered").into_iter().rfind(|e| e.thread.as_deref() == Some("main")).unwrap_or_else(|| panic!("{said:?}\n{}", r.f.rig.dump()));
    assert!(brief.data["sections"].as_array().unwrap().iter().any(|s| s == "digests"), "{}", brief.data);
}

#[test]
fn a_report_claim_that_names_no_line_is_unchecked_and_sends_the_item_back_until_the_bound_blocks_it_and_wakes_main() {
    let bad = report(Some("src/gone.rs:3"));
    let r = rig(false, &[&bad, &bad, &bad, &bad, &bad], checker(2), |_| {});
    r.f.front.say("main", LINE);
    r.until("the item was blocked", |r| r.item("c1").state == "blocked");
    assert_eq!(r.sent_back().len(), 3, "sent back as often as `plan.sends` allows, and no more: {}", r.f.rig.dump());
    let d = digests(&r);
    let blocked = d.iter().find(|d| d["state"] == "blocked").unwrap_or_else(|| panic!("no digest of the block: {d:?}\n{}", r.f.rig.dump()));
    assert!(blocked["text"].as_str().unwrap().contains("1 unchecked") && blocked["wake"] == true, "{blocked}");
    // Main is woken once, with the digest, and the plan does not replan: there is no approach to change
    r.until("Main was woken", |r| r.f.tasks().iter().any(|t| t.spec.goal.starts_with("J4n: 1 of your chats need a decision")));
    assert_eq!(r.f.rig.of_kind("plan.asked").len(), 0, "a plan that is only the chats has no approach to change");
}

#[test]
fn a_held_piece_is_checked_reviewed_on_another_doer_and_merged_and_the_digest_says_so() {
    let r = rig(true, &[&report(None)], checker(4), |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| r.item("c1").state == "merged");
    // reviewed, then the folder is checked before the lay-in, then after it
    assert_eq!(r.path("c1"), ["running", "reported", "reported", "checked", "checked", "reviewed", "reviewed", "reviewed", "merged"], "{}", r.f.rig.dump());
    assert_eq!(r.held.applied().len(), 1, "laid in once");
    let all = r.f.tasks();
    let review = all.iter().find(|t| t.spec.role.as_deref() == Some("review")).expect("a review");
    assert_eq!(review.spec.doer, "own", "not the writer and not Main");
    let merged = digests(&r).into_iter().find(|d| d["state"] == "merged").expect("a digest of the merge");
    assert!(merged["text"].as_str().unwrap().contains("piece merged") && merged["text"].as_str().unwrap().contains("1 file"), "{merged}");
    // merged work stays: a plan of chats has no approach to change, so nothing replans it, and a line to Main is its own turn, not a change to the plan
    // every task is over before the next line, or a loaded machine makes that line a steer
    for t in r.f.tasks() {
        r.f.rig.finished(&t.id);
    }
    r.f.front.say("main", "and now tidy the docs a little");
    r.step();
    assert_eq!(r.item("c1").state, "merged");
    assert_eq!(r.f.rig.of_kind("plan.asked").len(), 0, "{}", r.f.rig.dump());
}

#[test]
fn a_chat_that_writes_sent_back_goes_on_as_a_piece_in_a_worktree_and_is_merged_never_as_main_inline() {
    // the first report claims a line that is not there, so the chat is sent back; under `orchestrate` a rule may not start a piece, but a chat of a split
    // that writes is one: its retry is a piece of its own, and Main's checkout, where its files are not, never takes it
    // the brief is short and reads one file, so the cheap test alone would keep the retry inline
    let mut short = brief(true);
    short["paths"] = json!(["src/a/1.rs"]);
    let r = rig_of(short, true, &[&report(Some("src/gone.rs:3")), &report(None)], checker(6), |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| r.item("c1").state == "merged" || r.item("c1").state == "blocked");
    assert_eq!(r.sent_back().len(), 1, "{}", r.f.rig.dump());
    let routed: Vec<String> = r.f.rig.of_kind("plan.routed").into_iter().filter(|e| e.data["id"] == "c1").map(|e| e.data["place"].as_str().unwrap_or_default().to_string()).collect();
    assert!(!routed.iter().any(|w| w == "inline"), "the chat went back to Main's checkout: {routed:?}\n{}", r.f.rig.dump());
    let tries: Vec<String> = r.f.tasks().into_iter().filter(|t| t.spec.input["item"] == "c1" && t.spec.role.as_deref() == Some("piece")).map(|t| t.id).collect();
    assert_eq!(tries.len(), 1, "the retry is a piece: {}", r.f.rig.dump());
    assert_eq!(r.held.applied(), tries, "the retry is laid in, the unchecked try never: {}", r.f.rig.dump());
}

#[test]
fn a_chats_ran_claims_are_checked_against_its_own_runs_past_a_file_it_wrote_with_a_here_document() {
    // a chat has no parent task: its runs were never kept, so every command it said it ran was unchecked and its first report always went back
    let said = json!({ "answer": "built it", "found": [], "changed": "cli/", "ran": [{ "cmd": "go vet ./... (in cli/)", "exit": 0 }, { "cmd": "go test ./...", "exit": 0 }], "sure": "high", "open": [] }).to_string();
    let body = "package main\n".repeat(40);
    let line = format!("mkdir -p cli && cd cli && cat > main.go <<'EOF'\n{body}EOF\ngo vet ./... && go test ./... 2>&1 | tail -3");
    let pi = Scripted::with_caps("pi", caps(), vec![vec![Beat::Session("pi-s".into()), Beat::Shell(line, "ok".into(), 0), Beat::Say(said.clone()), end(&said)]]);
    let r = rig_with(brief(true), true, pi, checker(4), |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    assert_eq!(r.sent_back().len(), 0, "{}", r.f.rig.dump());
    let made = r.f.rig.of_kind("report.made");
    assert_eq!(made[0].data["report"]["ran"].as_array().unwrap().iter().filter(|x| x["unchecked"] == true).count(), 0, "{:?}", made[0].data);
}

#[test]
fn a_piece_whose_check_failed_is_never_laid_in() {
    let failing = Scripted::with_caps("check", caps(), vec![vec![Beat::Fail("tests failed".into())]; 6]);
    let r = rig(true, &[&report(None)], failing, |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was sent back", |r| !r.sent_back().is_empty());
    assert!(r.held.applied().is_empty(), "nothing was laid in: {}", r.f.rig.dump());
    assert!(r.sent_back()[0].data["result"].as_str().unwrap().contains("its checks failed"), "{:?}", r.sent_back());
    // and the piece is Main's now: its turn is told why
    r.until("Main took it", |r| r.f.tasks().iter().any(|t| t.spec.thread.as_deref() == Some("main") && t.spec.goal.contains("Sent back (1 of 3)")));
    assert!(r.held.applied().is_empty());
}

fn parts(n: usize) -> Parts {
    Parts { chat: format!("c{n}"), goal: "a goal ".repeat(40), place: "piece".into(), state: "merged".into(), result: "laid in; ".repeat(60), micros: 40_000, claims: (9, 2), files: (0..20).map(|i| format!("src/module_{i}/file.rs")).collect(), counts: "20 files +300 −40".into(), ran: (0..9).map(|i| (format!("cargo test --package crate{i} --lib"), 0)).collect(), sure: "mid".into(), open: (0..9).map(|i| format!("a long doubt number {i} about the retry loop and its reset")).collect() }
}

#[test]
fn a_digest_is_cut_to_its_cap_and_main_holds_the_newest_few_after_twenty_reports() {
    let long = line(&parts(1), 400);
    assert!(j4n_core::brief::tokens(&long) <= 400 && long.starts_with("c1 "), "{} tokens", j4n_core::brief::tokens(&long));
    assert!(j4n_core::brief::tokens(&line(&parts(1), 120)) <= 120);
    // twenty reports in: the Brief's section holds a line for each chat and the newest three digests, not what twenty digests would take
    let all: Vec<Digest> = (1..=20).map(|n| Digest { chat: format!("c{n}"), state: "merged".into(), text: line(&parts(n), 400), key: format!("c{n}:merged"), wake: false }).collect();
    let states: Vec<String> = (1..=20).map(|n| format!("c{n} a goal · piece merged")).collect();
    let text = section(&states, &all, 3).unwrap();
    assert!(j4n_core::brief::tokens(&text) <= 3 * 400 + 20 * 12 + 40, "{} tokens", j4n_core::brief::tokens(&text));
    assert!(text.contains("c20 ") && text.contains("c18 ") && !text.contains("c17 a goal a goal") && text.contains("(17 older in the Log)"), "{text}");
    assert!(section(&[], &[], 3).is_none());
}

/// [`rig_with`] with Main's own doer as given (its planning turn says what Main keeps).
fn rig_main(claude: Arc<Scripted>) -> Rig3 {
    rig_main_with(claude, doer("pi", &[&report(None)]))
}

/// [`rig_main`] with the chat's doer as given.
fn rig_main_with(claude: Arc<Scripted>, pi: Arc<Scripted>) -> Rig3 {
    let held = Held::new();
    let doers = vec![claude, pi, doer("own", &[CLEAN; 6]), checker(6), chatty("zcheap", &[&look(true), ONE])];
    let f: Fx = fx_placed(doers, Some("pi"), vec![held.place()], settings);
    Rig3 { f, held }
}

/// Main: the planning turn answers `plan`, its own part writes `core.py` when `writes`, and each turn after says what it did.
fn main_doer(plan: &str, writes: bool) -> Arc<Scripted> {
    let mut own = vec![Beat::Session("claude-s".into())];
    if writes {
        own.push(Beat::Put("core.py".into(), "class Game: pass\n".into()));
    }
    own.push(end("wrote core.py"));
    let turn = |said: &str| vec![Beat::Session("claude-s".into()), end(said)];
    Scripted::with_caps("claude", caps(), vec![turn(plan), own, turn("joined"), turn("joined"), turn("fixed"), turn("fixed")])
}

#[test]
fn main_is_told_its_own_part_of_a_split_and_when_the_chats_work_is_laid_in_it_is_asked_once_to_join_the_parts() {
    let plan = json!({ "chats": [brief(true)], "main": "I write core.py and the wiring" }).to_string();
    let r = rig_main(main_doer(&plan, false));
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    for _ in 0..3 {
        r.step();
    }
    let turns: Vec<_> = r.f.tasks().into_iter().filter(|t| t.spec.doer == "claude" && t.spec.role.as_deref() == Some("work")).collect();
    assert!(turns[0].spec.goal.contains("do your own part now") && turns[0].spec.goal.contains("As you planned it: I write core.py and the wiring"), "{}", turns[0].spec.goal);
    let join: Vec<_> = turns.iter().filter(|t| t.spec.goal.starts_with("The chats' parts are laid in")).collect();
    assert_eq!(join.len(), 1, "asked once: {}", r.f.rig.dump());
    let g = &join[0].spec.goal;
    assert!(g.contains("- c1: do the part and report (src/a/1.rs)") && g.contains("as you planned it: I write core.py and the wiring") && g.contains(&format!("The line was: {LINE}")), "{g}");
    assert_eq!(r.f.rig.of_kind("second.wire").len(), 1);
}

#[test]
fn main_s_own_part_of_a_split_that_changed_files_is_reviewed_on_another_doer_while_the_chats_work() {
    let plan = json!({ "chats": [brief(true)], "main": "I write core.py" }).to_string();
    let r = rig_main(main_doer(&plan, true));
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    for _ in 0..3 {
        r.step();
    }
    let started = r.f.rig.of_kind("second.started");
    let own = r.f.tasks().into_iter().find(|t| t.spec.goal.contains("As you planned it: I write core.py")).expect("Main's own part");
    assert!(started.iter().any(|e| e.data["of"] == own.id.as_str()), "Main's own part is reviewed: {}", r.f.rig.dump());
    let review = r.f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("review") && t.spec.parent.as_deref() == Some(own.id.as_str())).expect("its review");
    assert_ne!(review.spec.doer, "claude", "on another doer");
}

/// The plan fold's `parts_out` for Main's thread: whether a chat's part is on its way to be laid in.
fn parts_out(f: &Fx) -> j4n_core::value::Value {
    use j4n_core::fold::Fold;
    use j4n_core::value::Key;
    f.keeper.read(&[Key::Name("main".into()), Key::Name("parts_out".into())], 0)
}

/// The shipped `lane-check` rule's condition, for a turn of Main's on `main` that wrote a file, with the plan fold as `plan`.
fn lane_check_holds(plan: Value) -> bool {
    use j4n_core::eval::Scope;
    use j4n_core::value::Value as V;
    let rules: toml::Value = toml::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs/base/rules/check.toml")).unwrap()).unwrap();
    let rule = rules["rule"].as_array().unwrap().iter().find(|r| r["id"].as_str() == Some("lane-check")).unwrap();
    let when = j4n_core::expr::Expr::parse(rule["when"].as_str().unwrap()).unwrap();
    let scope = Scope::new()
        .with("ev", V::from_json(&json!({ "data": { "ok": true, "changed": { "created": ["core.py"], "modified": [], "deleted": [] } } })))
        .with("set", V::from_json(&json!({ "check": { "after": true } })))
        .with("task", V::text("t2"))
        .with("fold", V::from_json(&json!({ "tasks": { "t2": { "doer": "claude", "applied": false, "thread": "main" } }, "plan": plan })));
    when.holds(&scope)
}

#[test]
fn main_s_own_part_of_a_split_is_checked_with_the_chats_parts_when_it_joins_them_and_not_while_one_is_out() {
    // Main's tests may import a module a chat is still building (`import engine`, c1's): while a chat's part is out the plan says so,
    // and the lane check does not run on Main's turn; with none out, or no plan, it runs as before
    let plan = json!({ "chats": [brief(true)], "main": "I write core.py" }).to_string();
    let r = rig_main(main_doer(&plan, true));
    r.f.front.say("main", LINE);
    r.until("the chat works", |r| r.item("c1").state == "running");
    assert_eq!(parts_out(&r.f), j4n_core::value::Value::Bool(true), "{}", r.f.rig.dump());
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    assert_eq!(parts_out(&r.f), j4n_core::value::Value::Bool(false));
    assert!(!lane_check_holds(json!({ "main": { "parts_out": true } })), "Main's part waits for the join");
    assert!(lane_check_holds(json!({ "main": { "parts_out": false } })), "the join turn is checked");
    assert!(lane_check_holds(json!({})), "a turn with no plan is checked");
}

#[test]
fn a_chat_sent_back_and_retried_as_a_piece_is_still_a_chats_part_out_until_it_is_laid_in_and_then_main_joins_it() {
    // a retry is routed again (`via` = `delegate`); the part stays the chat's, so it is out while it runs and joined when it is laid in
    let plan = json!({ "chats": [brief(true)], "main": "I write core.py" }).to_string();
    let r = rig_main_with(main_doer(&plan, false), doer("pi", &[&report(Some("src/gone.rs:3")), &report(None)]));
    r.f.front.say("main", LINE);
    r.until("the chat was sent back", |r| !r.sent_back().is_empty());
    assert_eq!(parts_out(&r.f), j4n_core::value::Value::Bool(true), "{}", r.f.rig.dump());
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    for _ in 0..3 {
        r.step();
    }
    assert_eq!(r.item("c1").state, "merged", "{}", r.f.rig.dump());
    let wire = r.f.rig.of_kind("second.wire");
    assert_eq!(wire.len(), 1, "{}", r.f.rig.dump());
    assert_eq!(wire[0].data["laid"], json!(["c1"]));
}

#[test]
fn a_chats_part_laid_in_over_a_failure_the_folder_had_before_it_is_merged_and_main_s_join_is_told_what_still_fails() {
    // the folder's check fails on Main's own test before the part is laid in: the part must not be rewound for it
    let r = rig_with(brief(true), true, doer("pi", &[&report(None)]), folder_check(|_| Some(SCREEN_UNLOADED)), |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    for _ in 0..3 {
        r.step();
    }
    let c1 = r.item("c1");
    assert_eq!(c1.state, "merged", "{}", r.f.rig.dump());
    assert!(c1.result.contains("the folder's check still fails as before: unittest.loader._FailedTest.test_screen"), "{}", c1.result);
    let join = r.f.tasks().into_iter().find(|t| t.spec.goal.contains("Now join what is here")).unwrap_or_else(|| panic!("Main's join turn\n{}", r.f.rig.dump()));
    assert!(join.spec.goal.contains("still fails after the last part was laid in, as it failed before it: unittest.loader._FailedTest.test_screen") && join.spec.goal.contains("fix it as you join them"), "{}", join.spec.goal);
}

#[test]
fn a_chats_part_that_was_stopped_is_named_to_main_as_held_and_the_parts_that_are_here_are_still_joined() {
    // c1 passes its cap and its item is blocked: the plan must still end, and Main still be asked to join
    let plan = json!({ "chats": [brief(true)], "main": "I write core.py" }).to_string();
    let bad = report(Some("src/gone.rs:3"));
    let r = rig_main_with(main_doer(&plan, false), doer("pi", &[&bad, &bad, &bad, &bad, &bad]));
    r.f.front.say("main", LINE);
    r.until("the piece was stopped", |r| r.item("c1").state == "blocked");
    for _ in 0..3 {
        r.step();
    }
    let wire = r.f.rig.of_kind("second.wire");
    assert_eq!(wire.len(), 1, "{}", r.f.rig.dump());
    assert_eq!((wire[0].data["laid"].clone(), wire[0].data["held"].clone()), (json!([]), json!(["c1"])));
    let join = r.f.tasks().into_iter().find(|t| t.spec.goal.contains("These parts were stopped")).expect("Main's join turn");
    assert!(join.spec.goal.contains("- c1: do the part and report") && join.spec.goal.contains("do not write them again") && join.spec.goal.contains("Now join what is here"), "{}", join.spec.goal);
}

struct PiModels;

impl j4n_core::front::models::ModelLists for PiModels {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        (doer == "pi").then(|| ["pi-haiku-1", "pi-sonnet-1", "pi-opus-1"].iter().map(|m| m.to_string()).collect())
    }
}

/// The chat's task model when Main briefs `opus` and the ceiling is `top`.
fn chat_model(top: &str) -> (Option<String>, Vec<String>) {
    let mut b = brief(true);
    b["model"] = json!("opus");
    let plan = json!({ "chats": [b], "main": "I write core.py" }).to_string();
    let held = Held::new();
    let doers = vec![main_doer(&plan, false), doer("pi", &[&report(None)]), doer("own", &[CLEAN; 6]), checker(6), chatty("zcheap", &[&look(true), ONE])];
    let top = top.to_string();
    let f: Fx = fx_placed(doers, Some("pi"), vec![held.place()], move |c: &mut Cfg| {
        settings(c);
        c.models = Arc::new(PiModels);
        c.prices = j4n_core::front::tiers::parse(j4n_core::front::tiers::PRICES);
        c.orchestrate.chat_top = top;
    });
    f.front.say("main", LINE);
    let model = f.tasks().into_iter().find(|t| t.spec.doer == "pi").map(|t| t.spec.tune.model).unwrap_or_else(|| panic!("no chat started: {}", f.rig.dump()));
    (model, f.notes())
}

#[test]
fn a_chat_briefed_on_a_model_dearer_than_the_ceiling_runs_on_the_ceiling_and_says_so_and_with_no_ceiling_it_keeps_main_s_pick() {
    // Main may brief a chat on opus, which costs far more of a five-hour window than its part needs
    let (model, notes) = chat_model("sonnet");
    assert_eq!(model.as_deref(), Some("pi-sonnet-1"), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("`pi-opus-1` costs more than `orchestrate.chat_top` (sonnet): it runs on pi-sonnet-1")), "{notes:?}");
    let (model, notes) = chat_model("");
    assert_eq!(model.as_deref(), Some("pi-opus-1"), "{notes:?}");
    assert!(!notes.iter().any(|n| n.contains("chat_top")), "{notes:?}");
}

/// A run under `--approval full` whose one chat writes and whose work the folder refuses at laying in; Main answers every turn with `main`.
fn held_rig(main: &str, tune: impl FnOnce(&mut Cfg)) -> Rig3 {
    let held = Held::new();
    *held.refuse_all.lock().unwrap() = Some("src/a/1.rs changed in the folder".into());
    let (plan, rep) = (json!({ "chats": [brief(true)], "main": "" }).to_string(), report(None));
    let doers = vec![doer("claude", &[&plan, main, main, main, main, main, main]), doer("pi", &[rep.as_str(); 4]), doer("own", &[CLEAN; 6]), checker(8), chatty("zcheap", &[&look(true), ONE])];
    let f: Fx = fx_placed_full(doers, Some("pi"), vec![held.place()], |c| {
        settings(c);
        tune(c);
    });
    Rig3 { f, held }
}

#[test]
fn under_full_approval_main_s_word_lays_a_stopped_part_in_once_and_it_is_checked_and_not_put_to_main_again() {
    let r = held_rig("main worked\nheld c1: lay", |_| {});
    r.f.front.say("main", LINE);
    r.until("the part was blocked", |r| r.item("c1").state == "blocked");
    r.until("Main was woken", |r| r.f.tasks().iter().any(|t| t.spec.goal.starts_with("J4n: 1 of your chats need a decision") && t.spec.goal.contains("held <id>: lay")));
    r.until("Main's word was read", |r| !r.f.rig.of_kind("orchestrate.held").is_empty());
    let said = r.f.rig.of_kind("orchestrate.held");
    assert_eq!((said.len(), said[0].data["chat"].as_str(), said[0].data["do"].as_str()), (1, Some("c1"), Some("lay")), "{}", r.f.rig.dump());
    // the folder still refuses it: tried once more, then held again, and Main is not woken a second time for it
    r.until("blocked again", |r| r.path("c1").iter().filter(|s| *s == "blocked").count() >= 2);
    for _ in 0..4 {
        r.step();
    }
    let woken = r.f.tasks().iter().filter(|t| t.spec.goal.starts_with("J4n: 1 of your chats need a decision")).count();
    assert_eq!(woken, 1, "one decision per part: {}", r.f.rig.dump());
    assert_eq!(r.f.rig.of_kind("orchestrate.held").len(), 1);
    assert!(r.path("c1").contains(&"reviewed".to_string()), "laid in through the checks: {:?}", r.path("c1"));
}

#[test]
fn a_review_s_notes_do_not_send_a_chat_s_part_back_and_main_s_join_is_told_them() {
    let noted = json!({ "answer": "read the change", "sure": "mid", "found": [], "ran": [], "open": [], "notes": ["src/a/1.rs:4 would break if `step` were renamed"] }).to_string();
    let held = Held::new();
    let plan = json!({ "chats": [brief(true)], "main": "" }).to_string();
    // the review is on Main's doer, the one beside the writer here: its second turn
    let doers = vec![doer("claude", &[&plan, &noted, "main worked", "main worked"]), doer("pi", &[&report(None)]), doer("own", &[&noted]), checker(6), chatty("zcheap", &[&look(true), ONE])];
    let r = Rig3 { f: fx_placed(doers, Some("pi"), vec![held.place()], settings), held };
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    assert_eq!(r.item("c1").state, "merged", "{}", r.f.rig.dump());
    r.until("Main joined", |r| r.f.tasks().iter().any(|t| t.spec.goal.contains("Now join what is here")));
    let join = r.f.tasks().into_iter().find(|t| t.spec.goal.contains("Now join what is here")).unwrap();
    assert!(join.spec.goal.contains("The reviews left these points for you to weigh") && join.spec.goal.contains("- c1: src/a/1.rs:4 would break if `step` were renamed"), "{}", join.spec.goal);
}

#[test]
fn a_part_whose_lay_in_was_rewound_is_laid_in_again_on_main_s_word_and_its_files_land() {
    // a part rewound after its lay-in broke a test was still taken as laid in: Main's `held c1: lay` checked a folder that held nothing of it
    let after = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let judge: crate::scripted::Judge = Arc::new(move |_: &std::path::Path, spec: &j4n_core::task::TaskSpec| {
        let first_after = spec.place == "here" && spec.input["before"] != true && after.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        Some(if first_after { vec![Beat::Fail(crate::branch_validate::SCREEN_DRAW_FAILS.into())] } else { vec![end("6-unittest: passed")] })
    });
    let held = Held::new();
    let (plan, rep, main) = (json!({ "chats": [brief(true)], "main": "" }).to_string(), report(None), "main worked\nheld c1: lay");
    // the review is on Main's doer, the one beside the writer here: its second turn
    let doers = vec![doer("claude", &[&plan, CLEAN, main, main, main, main, main]), doer("pi", &[rep.as_str(); 4]), doer("own", &[CLEAN; 6]), Scripted::judging("check", judge, vec![]), chatty("zcheap", &[&look(true), ONE])];
    let f: Fx = fx_placed_full(doers, Some("pi"), vec![held.place()], |c| {
        settings(c);
        c.sends = 0;
    });
    *held.into.lock().unwrap() = Some(f.rig.project.path().to_path_buf());
    *held.home.lock().unwrap() = Some(f.rig.home.path().to_path_buf());
    let r = Rig3 { f, held };
    r.f.front.say("main", LINE);
    r.until("the part was blocked", |r| r.item("c1").state == "blocked");
    let task = r.item("c1").task;
    assert!(r.f.rig.of_kind("task.rewound").iter().any(|e| e.task.as_deref() == Some(task.as_str())), "its lay-in was rewound: {}", r.f.rig.dump());
    r.until("Main's word was read", |r| !r.f.rig.of_kind("orchestrate.held").is_empty());
    r.until("the part was merged", |r| r.item("c1").state == "merged" || r.path("c1").iter().filter(|s| *s == "blocked").count() >= 2);
    assert_eq!(r.item("c1").state, "merged", "{}", r.f.rig.dump());
    assert_eq!(r.held.applied(), [task.clone(), task.clone()], "taken from where it is held a second time: {}", r.f.rig.dump());
    assert!(r.f.rig.project.path().join(format!("chess/{task}.py")).is_file(), "its files are in the folder: {}", r.f.rig.dump());
}

#[test]
fn a_word_on_a_stopped_part_in_another_form_or_with_main_s_word_off_leaves_it_held() {
    for (main, tune) in [("main worked\naccept its work, but tests/__init__.py is mine", false), ("main worked\nheld c1: lay", true)] {
        let r = held_rig(main, |c| c.orchestrate.held_main = !tune);
        r.f.front.say("main", LINE);
        r.until("the part was blocked", |r| r.item("c1").state == "blocked");
        r.until("Main worked after the block", |r| r.f.tasks().iter().filter(|t| t.spec.doer == "claude").count() >= 3 && r.f.tasks().iter().all(|t| t.state.is_terminal()));
        for _ in 0..3 {
            r.step();
        }
        assert!(r.f.rig.of_kind("orchestrate.held").is_empty(), "{main} / off {tune}: {}", r.f.rig.dump());
        assert_eq!(r.path("c1").iter().filter(|s| *s == "blocked").count(), 1, "held as it was: {:?}", r.path("c1"));
        assert!(r.held.applied().is_empty());
    }
}

#[test]
fn a_chat_sent_back_keeps_the_cap_of_its_brief() {
    // the piece class has no cap of its own, so a chat's part sent back ran with none
    let mut short = brief(true);
    short["paths"] = json!(["src/a/1.rs"]);
    let r = rig_of(short, true, &[&report(Some("src/gone.rs:3")), &report(None)], checker(6), |_| {});
    r.f.front.say("main", LINE);
    r.until("the piece was merged", |r| matches!(r.item("c1").state.as_str(), "merged" | "blocked"));
    let all = r.f.tasks();
    let cap = all.iter().find(|t| t.spec.input["chat"] == "c1").and_then(|t| t.spec.limits.usd_soft).expect("the brief's cap");
    let retry = all.iter().find(|t| t.spec.input["item"] == "c1" && t.spec.role.as_deref() == Some("piece")).expect("the retry");
    assert_eq!(retry.spec.limits.usd_soft, Some(cap), "{}", r.f.rig.dump());
}

#[test]
fn the_review_of_the_join_reads_the_parts_the_chats_laid_in_and_is_given_time_for_them() {
    let plan = json!({ "chats": [brief(true)], "main": "I write the wiring" }).to_string();
    let turn = |said: &str, put: bool| {
        let mut t = vec![Beat::Session("claude-s".into())];
        if put {
            t.push(Beat::Put("wire.py".into(), "import a\n".into()));
        }
        t.push(end(said));
        t
    };
    let r = rig_main(Scripted::with_caps("claude", caps(), vec![turn(&plan, false), turn("nothing of mine", false), turn("joined", true), turn("fixed", false)]));
    r.f.front.say("main", LINE);
    let join = |r: &Rig3| r.f.tasks().into_iter().find(|t| t.spec.role.as_deref() == Some("review") && t.spec.input["item"] == "second");
    r.until("the join was reviewed", |r| join(r).is_some());
    let review = join(&r).unwrap();
    assert!(review.spec.goal.contains("The files it changed: wire.py. The parts it joined, which the chats wrote: src/a/1.rs.") && review.spec.goal.contains("You have 6m00s"), "{}", review.spec.goal);
    assert_eq!((review.spec.limits.wall_ms, review.spec.input["branch"]["paths"].clone()), (Some(6 * 60_000), json!(["wire.py", "src/a/1.rs"])));
}
