//! Orchestrate's group view: the screen says who printed what. The events of one moment of an orchestrate run (Main ships the change, the review on
//! another doer reads, the check stopped) go through the core's fold with the group view on, and the screen shows a head on every block, the
//! role and who serves it: `Main · claude sonnet 5.5`, `review · omp glm 5.3`, `check`, `read · haiku 5.5 + Decider`. The plain view of the
//! same events is the screen it was.

use crate::chat_rows::{Feed, screen};
use j4n_core::change::Actor;
use j4n_core::front::rows;
use j4n_proto::Row;
use serde_json::json;

/// The line `ship it`, read by a Decider and the quick model, then worked by Main on the thread's own doer.
fn ship(f: &mut Feed) {
    f.ev("chat.say", None, json!({ "text": "ship it" }));
    f.ev("thread.bound", None, json!({ "doer": "claude", "model": "claude-sonnet-5-5", "how": "pin" }));
    f.ev("chat.note", None, json!({ "text": "reading the line …", "tone": "dim", "read": "reading" }));
    let read = json!({ "kind": { "pick": "one", "by": "model:haiku 5.5" }, "goes_on": { "pick": false, "by": "decider:jev" } });
    f.after(12).ev("orchestrate.looked", None, json!({ "line": "ship it", "read": read, "ms": 11_600, "doer": "claude", "model": "claude-haiku-5-5", "usd": 0.0051, "decision": "one", "taken": "one", "chats": 0, "why": "one release of the change" }));
    f.ev("chat.note", None, json!({ "text": "read as one · model:haiku 5.5 + decider:jev · $0.0051 · 11.6s · one release of the change", "tone": "dim", "read": "read" }));
    f.ev("task.queued", Some("t1"), json!({ "spec": { "goal": "ship it", "doer": "claude", "role": "work", "place": "here", "tune": {} } }));
    f.ev("task.started", Some("t1"), json!({ "doer": "claude", "place": "here" }));
    f.call("t1", "Read", r#"{"file_path":"docs/TESTING.md"}"#);
    f.call("t1", "Bash", r#"{"command":"git status --short"}"#);
    f.after(58).ev("run.metrics", Some("t1"), json!({ "usd": 0.05, "tokens_in": 1, "tokens_out": 0 }));
}

/// The review of Main's change on another doer, and the check, beside Main in the same line.
fn beside(f: &mut Feed) {
    f.ev("task.queued", Some("t2"), json!({ "spec": { "goal": "review the change", "doer": "omp", "role": "review", "place": "here", "tune": { "model": "glm-5.3" } } }));
    f.ev("task.started", Some("t2"), json!({ "doer": "omp", "place": "here" }));
    f.call("t2", "Grep", r#"{"pattern":"json_out"}"#);
    f.ev("task.queued", Some("t3"), json!({ "spec": { "goal": "check", "doer": "check", "role": "check", "place": "here", "tune": {} }, "code": true }));
    f.ev("task.started", Some("t3"), json!({ "doer": "check", "place": "here" }));
}

fn group(f: &Feed) -> Vec<Row> {
    rows::fold_in(Default::default(), Some("Main".into()), f.all.iter()).rows()
}

/// Every word of every row, its cells and its detail.
fn words(rows: &[Row]) -> String {
    rows.iter().flat_map(|r| r.cells.iter().chain(r.detail.iter())).map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n")
}

fn line<'a>(s: &'a str, has: &str) -> &'a str {
    s.lines().find(|l| l.contains(has)).unwrap_or_else(|| panic!("no line has `{has}`:\n{s}"))
}

#[test]
fn every_block_has_a_head_with_its_role_and_who_serves_it_and_a_lone_lane_keeps_its_row() {
    let mut f = Feed::new();
    ship(&mut f);
    let rows = group(&f);
    let s = screen(rows.clone(), 120, 30);
    // Main is the only lane of the line and still has its row, headed by its role
    let main = line(&s, "Main · claude sonnet 5.5");
    assert!(main.contains('├') || main.contains('╰') || main.contains('└'), "the head is a row of the tree: {main}\n{s}");
    assert!(s.contains("TESTING.md") && s.contains("git status --short"), "{s}");
    // the read is a head of its own: who read it, what it was taken as, how long, what it cost, and why under it
    let read = line(&s, "read · haiku 5.5 + Decider");
    assert!(read.contains("one lane") && read.contains("11.6s") && read.contains("$0.0051"), "{read}");
    assert!(s.contains("one release of the change"), "{s}");
    // the notes the read made are said by its head: neither is a row of its own
    assert!(!s.contains("reading the line") && !s.contains("read as one"), "{s}");
    // a Decider is named by what it does, never by its inside name, on any row
    let all = words(&rows).to_lowercase();
    assert!(!all.contains("jev") && !all.contains("decider:") && !all.contains("model:"), "{all}");
    let narrow = screen(rows, 60, 20);
    assert!(narrow.lines().all(|l| l.chars().count() <= 60), "{narrow}");
    assert!(line(&narrow, "Main").contains("· claude"), "at 60 columns the head keeps who serves it:\n{narrow}");
    assert!(line(&narrow, "read").contains("Decider") || line(&narrow, "read").contains("haiku"), "{narrow}");
    // where the width is short, who serves it gives up its last words before the head gives it up
    let tight = screen(group(&f), 40, 20);
    let main = line(&tight, "Main");
    assert!(main.contains("Main · claude") && !main.contains("5.5") && main.contains("$0.05"), "{main}\n{tight}");
}

#[test]
fn the_roles_that_run_beside_main_are_headed_by_role_and_the_plain_view_is_as_it_was() {
    let mut f = Feed::new();
    ship(&mut f);
    beside(&mut f);
    let s = screen(group(&f), 120, 30);
    assert!(s.contains("review · omp glm 5.3"), "{s}");
    let check = line(&s, "check");
    assert!(!check.contains('·') && !check.contains(" C "), "a lane of code names no model and no letter: {check}\n{s}");
    // without the group view the lanes keep their letters and the read its note
    let plain = screen(rows::fold(f.all.iter()).rows(), 120, 30);
    assert!(!plain.contains("Main ·") && !plain.contains("review ·"), "{plain}");
    assert!(plain.contains("read as one") && plain.contains("B omp"), "{plain}");
}


/// An approval `q` of lane `task`'s last call, answered `answer` by `by`: the question, the Gate's verdict when the Gate answered, the answer.
fn approval(f: &mut Feed, task: &str, q: &str, cmd: &str, answer: &str, by: Actor) {
    let ask = json!({ "local": q, "question": format!("allow Bash: {cmd}?"), "shape": "approval", "action": format!("shell: {cmd}"), "risk": 50, "inband": true, "act": { "verb": "shell", "actor": "core", "target": cmd } });
    f.ev("task.queued", Some(q), json!({ "spec": { "goal": "allow", "doer": "human", "parent": task, "place": "here", "input": { "ask": ask } } }));
    if matches!(by, Actor::Rule(_)) {
        f.ev("gate.verdict", Some(task), json!({ "verb": "shell", "target": cmd, "verdict": if answer == "yes" { "allow" } else { "block" }, "why": "a read of the project" }));
    }
    let via = if by == Actor::Operator { "human" } else { "chain" };
    f.by("task.settled", Some(q), by.clone(), json!({ "ok": true, "answer": answer, "via": via }));
    f.by("ask.answered", Some(task), by, json!({ "ask": q, "answer": answer, "via": via }));
}

#[test]
fn an_approval_the_gate_or_you_answered_is_one_word_at_the_right_of_its_step_and_a_no_keeps_its_row() {
    let mut f = Feed::new();
    ship(&mut f);
    // the Gate allowed the open `git status --short`
    approval(&mut f, "t1", "q1", "git status --short", "yes", Actor::Rule("gate".into()));
    f.ev("run.tool_result", Some("t1"), json!({ "name": "Bash", "detail": "ok" }));
    // you allowed the release
    f.call("t1", "Bash", r#"{"command":"make release"}"#);
    approval(&mut f, "t1", "q2", "make release", "yes", Actor::Operator);
    // you said no to a push
    f.call("t1", "Bash", r#"{"command":"git push"}"#);
    approval(&mut f, "t1", "q3", "git push", "no", Actor::Operator);
    let rows = group(&f);
    for (cols, high) in [(120, 30), (60, 20)] {
        let s = screen(rows.clone(), cols, high);
        assert!(line(&s, "git status --short").contains("✓ allowed"), "{s}");
        assert!(line(&s, "make release").contains("✓ you"), "{s}");
        // the question and the Gate's verdict are no rows of their own once answered yes
        assert!(!s.contains("allow Bash: git status") && !s.contains("allow Bash: ./scripts") && !s.contains("allow shell"), "{s}");
        // a no keeps its row, which says it
        assert!(line(&s, "allow Bash: git push").contains("no"), "{s}");
    }
    // the plain view keeps every row
    let plain = screen(rows::fold(f.all.iter()).rows(), 120, 30);
    assert!(plain.contains("allow Bash: git status --short?") && plain.contains("allow shell git status --short") && !plain.contains("✓ allowed"), "{plain}");
}

/// The board's moment: Main ended its work for the first line and the review of it on another doer still works when `ship it` is typed.
fn moment() -> Feed {
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "add a --json flag to the report command" }));
    f.ev("thread.bound", None, json!({ "doer": "claude", "model": "claude-sonnet-5-5", "how": "pin" }));
    f.ev("task.queued", Some("t0"), json!({ "spec": { "goal": "add the flag", "doer": "claude", "role": "work", "place": "here", "tune": {} } }));
    f.ev("task.started", Some("t0"), json!({ "doer": "claude", "place": "here" }));
    f.call("t0", "Edit", r#"{"file_path":"report.rs","old_string":"x"}"#);
    f.after(60).ev("task.settled", Some("t0"), json!({ "ok": true }));
    f.ev("task.queued", Some("t2"), json!({ "spec": { "goal": "review the change", "doer": "omp", "role": "review", "place": "here", "tune": { "model": "glm-5.3" } } }));
    f.ev("task.started", Some("t2"), json!({ "doer": "omp", "place": "here" }));
    f.call("t2", "Read", r#"{"file_path":"report.rs"}"#);
    f.ev("run.tool_result", Some("t2"), json!({ "name": "Read", "detail": "ok" }));
    f.call("t2", "Grep", r#"{"pattern":"json_out"}"#);
    f.after(60);
    ship(&mut f);
    f.call("t2", "Read", r#"{"file_path":"tests/report.rs"}"#);
    f
}

fn at(s: &str, has: &str) -> usize {
    s.lines().position(|l| l.contains(has)).unwrap_or_else(|| panic!("no line has `{has}`:\n{s}"))
}

#[test]
fn the_roles_that_run_at_once_are_side_lanes_of_the_newest_line_main_open_and_the_others_folded_to_their_last_step() {
    let f = moment();
    for (cols, high) in [(120, 30), (80, 24), (60, 24)] {
        let s = screen(group(&f), cols, high);
        // the review still works: it moved under the newest line, after Main, and says which line it works for
        let second = at(&s, "ship it");
        let rest = s.lines().skip(second).collect::<Vec<_>>().join("\n");
        let (main, review) = (second + at(&rest, "Main · claude"), second + at(&rest, "review"));
        assert!(main < review, "{s}");
        let head = s.lines().nth(review).unwrap_or_default();
        assert!(head.contains("of 12:29") && head.trim_end().ends_with('▸'), "folded, with the line it works for: {head}\n{s}");
        // folded to its last step: what it reads now, and none of what it did before
        assert!(s.lines().nth(review + 1).unwrap_or_default().contains("tests/report.rs"), "{s}");
        assert!(!s.contains("json_out"), "{s}");
        // Main's work for the line is open
        assert!(s.contains("TESTING.md") && s.contains("git status --short"), "{s}");
        // the first line keeps the role that ended there, and the review is no row of it any more
        let first = &s.lines().take(second).collect::<Vec<_>>().join("\n");
        assert!(first.contains("Main · claude") && !first.contains("review"), "{first}");
        assert!(s.lines().all(|l| l.chars().count() <= cols as usize), "{s}");
    }
    // the plain view leaves the review where it began
    let plain = screen(rows::fold(f.all.iter()).rows(), 120, 30);
    assert!(at(&plain, "B omp") < at(&plain, "ship it"), "{plain}");
}

#[test]
fn the_word_goes_only_on_the_call_the_approval_is_about_and_a_read_with_it_is_not_merged() {
    let mut f = Feed::new();
    ship(&mut f);
    f.ev("run.tool_result", Some("t1"), json!({ "name": "Bash", "detail": "ok" }));
    // two reads in a row merge into one row, unless one of them carries the word
    f.call("t1", "Read", r#"{"file_path":"a.md"}"#).call("t1", "Read", r#"{"file_path":"b.md"}"#);
    approval(&mut f, "t1", "q1", "b.md", "yes", Actor::Rule("gate".into()));
    // an approval of something no open call names keeps its row
    approval(&mut f, "t1", "q2", "rm -rf build", "yes", Actor::Rule("gate".into()));
    let s = screen(group(&f), 120, 30);
    assert!(line(&s, "b.md").contains("✓ allowed") && !line(&s, "b.md").contains("a.md"), "{s}");
    assert!(line(&s, "allow Bash: rm -rf build?").contains("yes"), "{s}");
}

#[test]
fn a_log_written_before_the_reads_notes_were_marked_keeps_its_notes_and_makes_no_second_head() {
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "ship it" }));
    f.ev("chat.note", None, json!({ "text": "reading the line …", "tone": "dim" }));
    let read = json!({ "kind": { "pick": "one", "by": "model:haiku 5.5" } });
    f.ev("orchestrate.looked", None, json!({ "line": "ship it", "read": read, "ms": 900, "taken": "one", "chats": 0 }));
    f.ev("chat.note", None, json!({ "text": "read as one · model:haiku 5.5 · 0.9s", "tone": "dim" }));
    let s = screen(group(&f), 120, 20);
    assert!(s.contains("reading the line") && s.contains("read as one") && !s.contains("read · haiku"), "{s}");
}

#[test]
fn the_lines_own_work_is_mains_even_when_the_binding_is_no_row_any_more() {
    // after `:clear` the binding of the thread is cut from the rows: Main's lane is still Main's, and the side lanes still open it first
    let mut f = Feed::new();
    f.ev("chat.say", None, json!({ "text": "ship it" }));
    f.ev("task.queued", Some("t1"), json!({ "spec": { "goal": "ship it", "doer": "claude", "role": "work", "place": "here", "tune": { "model": "claude-sonnet-5-5" } } }));
    f.ev("task.started", Some("t1"), json!({ "doer": "claude", "place": "here" }));
    f.call("t1", "Read", r#"{"file_path":"docs/TESTING.md"}"#);
    beside(&mut f);
    let s = screen(group(&f), 120, 30);
    let main = line(&s, "Main · claude sonnet 5.5");
    assert!(main.trim_end().ends_with('▾') || !main.contains('▸'), "Main is the open one: {main}\n{s}");
    assert!(s.contains("TESTING.md") && line(&s, "review · omp").trim_end().ends_with('▸'), "{s}");
}

#[test]
fn a_doer_that_names_its_call_by_a_description_gets_the_word_on_the_command_it_reports_after() {
    let mut f = Feed::new();
    ship(&mut f);
    beside(&mut f);
    // the review names its call by what it is for, and reports the command apart once it has run
    f.call("t2", "bash", r#"{"description":"Viewing the diff of the last commit"}"#);
    approval(&mut f, "t2", "q1", "git diff HEAD~1 --stat", "yes", Actor::Rule("gate".into()));
    f.ev("run.shell", Some("t2"), json!({ "cmd": "git diff HEAD~1 --stat", "out": " 2 files changed", "exit": 0 }));
    let rows = group(&f);
    let shell = rows.iter().find(|r| r.cells.first().is_some_and(|c| c.text == "git diff HEAD~1 --stat")).expect("the command's row");
    let marks: Vec<(&str, &str)> = shell.cells.iter().skip(2).map(|c| (c.icon.as_str(), c.text.as_str())).collect();
    assert_eq!(marks, vec![("yes", "allowed")], "one word, no bare tick beside it");
    assert!(!words(&rows).contains("allow Bash: git diff"), "{}", words(&rows));
}
