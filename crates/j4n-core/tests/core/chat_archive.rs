//! The archive of the chats. The fold is the only state (`chat.archived`, `chat.unarchived`, `chat.removed`); `:archive` and
//! `/archive` pick by rules with no model; a chat that runs is not archived; a line is never routed to an archived chat; a chat whose plan is done and
//! shown to hold is archived by itself; an archived chat is removed after `chats.archive.keep`, its doer's session files with it and the Log's rows
//! staying. The screen's side (the closed row, the open row, the key) is `j4n-tui`'s `chat_cases` and the tty golden.

use crate::front_cases::{Fx, fx_deciding};
use crate::scripted::{Beat, Scripted, end, say};
use crate::world::Scratch;
use j4n_core::change::Actor;
use j4n_core::front::Cfg;
use j4n_core::front::archive::Archive;
use j4n_core::log::New;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const DAY: u64 = 86_400_000;
const A: &str = "fix trailing commas in parser.rs";
const B: &str = "write the release notes for lexer.rs";
const C: &str = "benchmark the allocator under heavy load";

fn auto(c: &mut Cfg) {
    c.chats.on = true;
    c.chats.archive = Archive { auto: true, idle_ms: DAY, keep_ms: 14 * DAY };
}

fn turns(n: usize) -> Arc<Scripted> {
    Scripted::new("claude", (0..n).map(|i| vec![Beat::Session(format!("sess-{i}")), say(&format!("answer {i}")), end(&format!("answer {i}"))]).collect())
}

fn fx(n: usize, cfg: fn(&mut Cfg)) -> Fx {
    fx_deciding(vec![turns(n)], Some("absent"), cfg)
}

/// `(chat, archived)` of the pinned list's top rows.
fn list(f: &Fx) -> Vec<(String, bool)> {
    f.front.chat_list().iter().filter(|r| r.parent.is_none()).map(|r| (r.key.clone(), r.data.as_ref().unwrap()["archived"].as_bool().unwrap_or(false))).collect()
}

fn plan_item(f: &Fx, chat: &str, state: &str, unproven: bool) {
    let add = json!({ "op": "add", "id": "a", "goal": "do a", "done": "a works", "check": "", "reads": [], "writes": [], "depends": [], "where": "inline", "main": false, "task": "" });
    f.rig.log.append(New::of("plan.item", Actor::Core, add).thread(chat)).unwrap();
    let set = json!({ "op": "state", "id": "a", "state": state, "result": "ok", "unproven": unproven });
    f.rig.log.append(New::of("plan.item", Actor::Core, set).thread(chat)).unwrap();
}

#[test]
fn a_named_chat_is_archived_and_taken_out_again_and_the_list_puts_it_last() {
    let f = fx(2, auto);
    f.turn(A);
    f.turn(B);
    assert_eq!(list(&f), [("c2".to_string(), false), ("c1".to_string(), false)]);
    f.say(":archive c2");
    assert_eq!(list(&f), [("c1".to_string(), false), ("c2".to_string(), true)], "archived: after the others: {}", f.rig.dump());
    let ev = f.rig.of_kind("chat.archived");
    assert_eq!((ev.len(), ev[0].thread.as_deref(), ev[0].data["by"].as_str()), (1, Some("c2"), Some("you")));
    f.say(":unarchive c2");
    assert_eq!(list(&f), [("c2".to_string(), false), ("c1".to_string(), false)]);
    assert!(f.notes().iter().any(|n| n.contains("c2 is back in the list")), "{:?}", f.notes());
    // the one chat of a thread that is not archived is not taken out, and the main chat is never archived
    f.say(":unarchive c1");
    f.say(":archive main");
    assert!(f.notes().iter().any(|n| n.contains("c1 is not archived")) && f.notes().iter().any(|n| n.contains("the main chat is never archived")), "{:?}", f.notes());
}

#[test]
fn a_chat_that_runs_is_not_archived_and_the_refusal_says_why_in_one_line() {
    let two = Scripted::new("claude", vec![vec![Beat::Session("s".into()), Beat::Hold, end("late")]]);
    let f = fx_deciding(vec![two], Some("absent"), auto);
    f.say(A);
    assert!(f.tasks().iter().any(|r| !r.state.is_terminal()), "the turn runs");
    let said = f.say(":archive c1");
    assert!(matches!(&said, j4n_core::front::Said::Refused(w) if w == "c1 is running: nothing is archived while it does"), "{said:?}");
    assert_eq!(f.count("chat.archived"), 0);
    assert_eq!(list(&f), [("c1".to_string(), false)]);
}

#[test]
fn slash_archive_picks_by_rules_alone_a_chat_that_is_over_ok_and_idle_and_asks_no_model() {
    let hold = vec![Beat::Session("h".into()), Beat::Hold, end("late")];
    let one = |i: usize| vec![Beat::Session(format!("sess-{i}")), say("done"), end("done")];
    let f = fx_deciding(vec![Scripted::new("claude", vec![one(0), one(1), hold])], Some("absent"), auto);
    f.turn(A);
    f.rig.clock.advance(25 * 3_600_000);
    f.turn(B);
    f.say_new(C);
    let asked = f.decided.load(std::sync::atomic::Ordering::SeqCst);
    let said = f.say("/archive");
    assert!(matches!(said, j4n_core::front::Said::Command(ref c) if c == "archive"), "{said:?}");
    assert_eq!(list(&f), [("c3".to_string(), false), ("c2".to_string(), false), ("c1".to_string(), true)], "only c1: c2 was touched a minute ago, c3 runs: {}", f.rig.dump());
    let ev = f.rig.of_kind("chat.archived");
    assert_eq!((ev.len(), ev[0].data["by"].as_str()), (1, Some("rules")));
    assert!(f.notes().iter().any(|n| n.starts_with("archiving c1 · over, ok and untouched for 1d")), "one line says it first: {:?}", f.notes());
    assert_eq!(f.decided.load(std::sync::atomic::Ordering::SeqCst), asked, "no model was asked");
    // nothing more to take: said in a line
    f.say("/archive");
    assert!(f.notes().iter().any(|n| n.starts_with("nothing to archive")), "{:?}", f.notes());
    assert_eq!(f.count("chat.archived"), 1);
}

#[test]
fn a_chat_with_an_open_plan_item_or_a_failed_turn_is_not_taken_by_the_rules() {
    let bad = Scripted::new("claude", vec![vec![Beat::Session("s0".into()), say("ok"), end("ok")], vec![Beat::Session("s1".into()), Beat::Fail("boom".into())]]);
    let f = fx_deciding(vec![bad], Some("absent"), auto);
    f.turn(A);
    f.turn(B);
    plan_item(&f, "c1", "todo", false);
    f.rig.clock.advance(2 * DAY);
    f.say("/archive");
    assert_eq!(f.count("chat.archived"), 0, "c1 has an item open, c2's turn failed: {}", f.rig.dump());
    // naming the failed one is the operator's call
    f.say(":archive c2");
    assert_eq!(f.count("chat.archived"), 1);
}

#[test]
fn a_line_is_not_routed_to_an_archived_chat_it_opens_a_new_one() {
    let f = fx(3, auto);
    f.turn(A);
    f.turn("also fix the comma in parser.rs again");
    assert_eq!(f.rig.of_kind("chat.routed").iter().map(|e| e.data["to"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["c1", "c1"], "it goes to the chat that has the file");
    f.say(":archive c1");
    f.turn("and parser.rs once more");
    let routed = f.rig.of_kind("chat.routed");
    let last = &routed.last().unwrap().data;
    assert_eq!((last["to"].as_str(), last["new"].as_bool()), (Some("c2"), Some(true)), "{last}");
    assert_eq!(list(&f), [("c2".to_string(), false), ("c1".to_string(), true)]);
}

#[test]
fn a_line_typed_in_an_archived_chat_brings_it_back() {
    let f = fx(2, auto);
    f.turn(A);
    f.say(":archive c1");
    assert_eq!(list(&f), [("c1".to_string(), true)]);
    f.front.say("c1", "one more thing in parser.rs");
    assert_eq!(list(&f), [("c1".to_string(), false)], "the fold takes it back with the line");
}

#[test]
fn a_chat_whose_plan_is_done_and_shown_to_hold_is_archived_by_itself_after_half_an_hour_and_one_that_is_not_is_left() {
    let f = fx(2, auto);
    f.turn(A);
    f.turn(B);
    plan_item(&f, "c1", "merged", false);
    plan_item(&f, "c2", "merged", true);
    f.rig.clock.advance(29 * 60_000);
    f.front.archive_sweep();
    assert_eq!(f.count("chat.archived"), 0, "29 minutes: it is not folded yet");
    f.rig.clock.advance(2 * 60_000);
    f.front.archive_sweep();
    let ev = f.rig.of_kind("chat.archived");
    assert_eq!(ev.iter().map(|e| (e.thread.clone().unwrap(), e.data["by"].as_str().unwrap().to_string())).collect::<Vec<_>>(), [("c1".to_string(), "auto".to_string())], "c2's item is unproven, a plain chat has no proof");
    // the switch off: nothing is archived by itself
    let g = fx(1, |c| {
        auto(c);
        c.chats.archive.auto = false;
    });
    g.turn(A);
    plan_item(&g, "c1", "merged", false);
    g.rig.clock.advance(DAY);
    g.front.archive_sweep();
    assert_eq!(g.count("chat.archived"), 0);
}

#[test]
fn an_archived_chat_is_removed_after_keep_with_its_session_files_and_the_log_keeps_its_rows() {
    let home = Scratch::new("homes");
    let (claude, other): (PathBuf, PathBuf) = (home.path().join(".claude/projects/p"), home.path().join(".claude/projects/p"));
    std::fs::create_dir_all(&claude).unwrap();
    let (mine, not_mine) = (claude.join("sess-0.jsonl"), other.join("sess-9.jsonl"));
    std::fs::write(&mine, "x").unwrap();
    std::fs::write(&not_mine, "y").unwrap();
    let root = home.path().to_path_buf();
    let f = fx_deciding(vec![turns(2)], Some("absent"), move |c| {
        auto(c);
        let root = root.clone();
        c.homes = Some(Arc::new(move |_| (root.join(".claude"), root.clone())));
    });
    f.turn(A);
    f.say(":archive c1");
    f.rig.clock.advance(13 * DAY);
    f.front.archive_sweep();
    assert!(mine.exists() && f.count("chat.removed") == 0, "13 days: it is kept");
    f.rig.clock.advance(2 * DAY);
    f.front.archive_sweep();
    assert!(!mine.exists(), "its own session file is gone");
    assert!(not_mine.exists(), "a file that is not the id of one of its sessions is not touched");
    let removed = f.rig.of_kind("chat.removed");
    assert_eq!(removed.len(), 1, "{}", f.rig.dump());
    assert_eq!((removed[0].data["deleted"].as_u64(), removed[0].data["sessions"][0]["id"].as_str()), (Some(1), Some("sess-0")), "{}", removed[0].data);
    assert!(removed[0].data["paths"][0].as_str().unwrap().ends_with("sess-0.jsonl"));
    assert_eq!(list(&f), Vec::<(String, bool)>::new(), "gone from every list, the archive included");
    assert_eq!(f.rig.of_kind("chat.opened").len(), 1, "the Log's rows stay: it is a chain");
    // its id is not given again
    f.turn(B);
    assert_eq!(list(&f), [("c2".to_string(), false)]);
}

#[test]
fn keep_zero_removes_nothing() {
    let f = fx(1, |c| {
        auto(c);
        c.chats.archive.keep_ms = 0;
    });
    f.turn(A);
    f.say(":archive c1");
    f.rig.clock.advance(400 * DAY);
    f.front.archive_sweep();
    assert_eq!((f.count("chat.removed"), list(&f)), (0, vec![("c1".to_string(), true)]));
}
