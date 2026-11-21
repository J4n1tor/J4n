//! The small parts of the front on their own: the phrase table, the rule that reads it, the binding of a thread as a fold, and
//! the chat as lines.

use crate::front_cases::base;
use crate::roundtrip::agree;
use crate::world::{Scratch, open_log};
use j4n_core::answer::{Answerer, Offer, Question};
use j4n_core::ask::{Answer, AskShape};
use j4n_core::change::Actor;
use j4n_core::fold::{Fold, Folds};
use j4n_core::front::chat::Chat;
use j4n_core::front::phrase::{Cmd, Phrase, Phrases, intake_rule};
use j4n_core::front::thread::{Binding, Binds};
use j4n_core::log::{Ev, New};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Item, Role, Tone};
use serde_json::{Value, json};

// ---- the phrase table ----

fn table(text: &str) -> (Phrases, Vec<String>) {
    let layer = Layer::new(LayerKind::Pack, "p", vec![SourceFile { path: "phrases.toml".into(), text: format!("api = 1\n{text}") }]);
    let (tables, problems) = Tables::load(vec![layer]);
    assert!(problems.is_empty(), "{problems:?}");
    let (phrases, found) = Phrases::load(&tables);
    (phrases, found.iter().map(ToString::to_string).collect())
}

fn phrase(id: &str, words: &str, command: &str) -> String {
    format!("[[phrase]]\nid = \"{id}\"\nwords = {words}\ncommand = \"{command}\"\n")
}

fn real() -> Phrases {
    let t = base();
    let (p, found) = Phrases::load(&t);
    assert!(found.is_empty(), "{found:?}");
    p
}

#[test]
fn the_phrase_kind_reads_the_same_in_the_schema_and_in_the_struct() {
    let (t, problems) = crate::world::load(vec![]);
    assert!(problems.is_empty());
    assert_eq!(agree::<Phrase>(&t).unwrap(), 16);
}

#[test]
fn the_base_commands_are_found_by_their_words_in_any_case_with_what_follows_as_arguments() {
    let p = real();
    let cmd = |name: &str, args: &[&str]| Some(Cmd { name: name.into(), args: args.iter().map(|a| a.to_string()).collect() });
    assert_eq!(p.parse(":fresh"), cmd("fresh", &[]));
    // `:new` is a command of its own now: the table of phrases does not know it, the commands claim it; `:main` is a way to the top, never listed
    assert_eq!(p.parse("  :NEW  "), None);
    assert_eq!(p.parse(":Main"), cmd("top", &[]));
    assert_eq!(p.parse(":switch claude haiku"), cmd("switch", &["claude", "haiku"]));
    assert_eq!(p.parse(":Doer\tx"), cmd("switch", &["x"]));
    for (words, name) in [(":stop", "stop"), (":abort", "stop"), (":kill", "kill"), (":status", "status"), (":help", "help"), (":?", "help"), (":quit", "quit"), (":q", "quit"), (":exit", "quit")] {
        assert_eq!(p.parse(words), cmd(name, &[]), "{words}");
    }
}

#[test]
fn a_line_that_is_not_a_command_is_not_one_whatever_it_looks_like() {
    let p = real();
    for line in ["", "   ", "fresh", "please :fresh", ": fresh", ":freshen", ":nosuch", "/plan do it", "switch claude", "::fresh"] {
        assert_eq!(p.parse(line), None, "{line:?}");
    }
}

#[test]
fn the_help_lists_the_first_word_of_every_phrase_with_what_it_does() {
    let docs = real();
    let docs = docs.docs();
    assert_eq!(docs.len(), 14, "the thirteen of the colon words and `:ask`; a phrase that is only wordings has no word to list");
    assert!(docs.iter().any(|(w, d)| w == ":fresh" && d.contains("start the thread again")), "{docs:?}");
}

#[test]
fn a_word_two_phrases_claim_goes_to_the_first_and_is_reported() {
    let (p, found) = table(&format!("{}{}", phrase("a", r#"[":x"]"#, "fresh"), phrase("b", r#"[":X", ":y"]"#, "stop")));
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("`:X`") && found[0].contains("already"), "{found:?}");
    assert_eq!(p.parse(":x").map(|c| c.name), Some("fresh".into()));
    assert_eq!(p.parse(":y").map(|c| c.name), Some("stop".into()));
}

#[test]
fn a_word_with_no_colon_or_with_a_space_in_it_is_refused() {
    let (p, found) = table(&phrase("a", r#"["fresh", ":two words", ":ok"]"#, "fresh"));
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|f| f.contains("colon")));
    assert_eq!(p.parse(":ok").map(|c| c.name), Some("fresh".into()));
    assert_eq!(p.parse("fresh"), None);
}

#[test]
fn a_command_the_core_does_not_have_is_refused_by_the_kind() {
    let layer = Layer::new(LayerKind::Pack, "p", vec![SourceFile { path: "phrases.toml".into(), text: format!("api = 1\n{}", phrase("a", r#"[":x"]"#, "explode")) }]);
    let (tables, _) = Tables::load(vec![layer]);
    assert!(tables.ids("phrase").is_empty(), "the row was not loaded");
}

fn question(text: &str) -> Question {
    Question { ask: "i#1".into(), point: "intake".into(), asker: "front".into(), thread: None, text: text.into(), shape: AskShape::Text, goal: text.into(), act: None }
}

#[test]
fn the_intake_rule_calls_a_command_a_command_and_leaves_every_other_line_alone() {
    let rule = intake_rule(real());
    assert_eq!((rule.id(), rule.kind()), ("rule:intake", j4n_core::answer::Kind::Rule));
    match rule.offer(&question(":fresh")) {
        Offer::Answer { answer, conf, why } => {
            assert_eq!((answer, conf), (Answer::Pick { id: "command".into() }, 1.0));
            assert!(why.contains("fresh"), "{why}");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(rule.offer(&question("what is in here?")), Offer::Pass { .. }));
}

// ---- the binding of a thread ----

fn bound(thread: &str, doer: &str, model: Option<&str>, how: &str) -> New {
    New::of("thread.bound", Actor::Core, json!({ "doer": doer, "model": model, "how": how })).thread(thread)
}

#[test]
fn a_thread_is_bound_until_it_is_made_fresh_and_threads_do_not_touch_each_other() {
    let s = Scratch::new("binds");
    let (log, _) = open_log(&s);
    let folds = Folds::new(&s.join(".j4n/folds"));
    folds.attach(&log).unwrap();
    let binds = Binds::new();
    assert!(folds.add(Box::new(binds.clone()), log.head()).is_empty());
    log.append(bound("a", "claude", Some("haiku"), "pinned")).unwrap();
    log.append(bound("b", "test", None, "elected")).unwrap();
    assert_eq!(binds.get("a"), Some(Binding { doer: "claude".into(), model: Some("haiku".into()), endpoint: None, account: None, how: "pinned".into(), answer: None, lines: 0, tasks: 0 }));
    assert_eq!(binds.get("b").map(|b| (b.doer, b.model)), Some(("test".into(), None)));
    log.append(bound("a", "test", None, "switch")).unwrap();
    assert_eq!(binds.get("a").map(|b| (b.doer, b.model, b.how)), Some(("test".into(), None, "switch".into())), "the last binding stands whole: the old model is gone");
    log.append(New::of("thread.fresh", Actor::Operator, json!({})).thread("a")).unwrap();
    assert_eq!(binds.get("a"), None);
    assert!(binds.get("b").is_some());
    assert_eq!(binds.get("nope"), None);
}

#[test]
fn events_that_do_not_make_a_binding_are_skipped_and_the_fold_replays_to_the_same_state() {
    let s = Scratch::new("binds2");
    let (log, _) = open_log(&s);
    let folds = Folds::new(&s.join(".j4n/folds"));
    folds.attach(&log).unwrap();
    let binds = Binds::new();
    folds.add(Box::new(binds.clone()), log.head());
    log.append(New::of("thread.bound", Actor::Core, json!({ "how": "elected" })).thread("a")).unwrap();
    log.append(New::of("thread.bound", Actor::Core, json!({ "doer": "", "how": "elected" })).thread("a")).unwrap();
    log.append(New::of("thread.bound", Actor::Core, json!({ "doer": "claude", "how": "elected" }))).unwrap();
    log.append(New::of("thread.fresh", Actor::Core, json!({}))).unwrap();
    log.append(New::of("thread.somethingnew", Actor::Core, json!({ "doer": "x" })).thread("a")).unwrap();
    assert_eq!(binds.get("a"), None, "no doer, an empty doer, no thread: none of them binds");
    log.append(bound("a", "claude", Some("m"), "pinned")).unwrap();
    log.append(bound("b", "test", None, "switch")).unwrap();
    // the cut is the newest task the thread queued when `:fresh` was said, and the replay finds the same one
    log.append(New::of("task.queued", Actor::Operator, json!({})).task("t1").thread("c")).unwrap();
    log.append(New::of("task.queued", Actor::Operator, json!({})).task("t2").thread("c")).unwrap();
    log.append(New::of("thread.fresh", Actor::Operator, json!({})).thread("c")).unwrap();
    log.append(New::of("task.queued", Actor::Operator, json!({})).task("t3").thread("c")).unwrap();
    assert_eq!((binds.cut("c"), binds.get("c"), binds.cut("b")), (Some("t2".into()), None, None));
    let replay = Binds::new();
    let mut fold = replay.clone();
    for e in log.read(0) {
        if fold.wants(&e.kind) {
            fold.apply(&e);
        }
    }
    assert_eq!(fold.save(), binds.save());
    assert_eq!(replay.cut("c"), Some("t2".into()), "the replay holds the cut too");
    let mut loaded = Binds::new();
    loaded.load(binds.save()).unwrap();
    assert_eq!(loaded.get("a"), binds.get("a"));
    assert!(loaded.load(json!("not a map")).is_err());
    loaded.reset();
    assert_eq!(loaded.get("a"), None);
    assert_eq!((binds.id(), binds.version()), ("threads", 9));
    let read = binds.read(&[j4n_core::value::Key::Name("a".into()), j4n_core::value::Key::Name("doer".into())], 0);
    assert_eq!(read, j4n_core::value::Value::text("claude"));
}

// ---- the chat as lines ----

fn ev(seq: u64, kind: &str, data: Value) -> Ev {
    Ev { seq, at_ms: 1, kind: kind.into(), task: Some("t1".into()), thread: Some("main".into()), by: Actor::Core, cause: None, class: Default::default(), data }
}

fn one(kind: &str, data: Value) -> (Role, Tone, String) {
    let items = Chat::new().items(&ev(7, kind, data));
    assert_eq!(items.len(), 1, "{kind}: {items:?}");
    let Item::Put { line } = &items[0] else { panic!("{items:?}") };
    assert_eq!((line.key.as_str(), line.task.as_deref(), line.partial), ("s7", Some("t1"), false));
    (line.role, line.tone, line.text.clone())
}

fn none(kind: &str, data: Value) {
    let items = Chat::new().items(&ev(7, kind, data));
    assert!(items.is_empty(), "{kind}: {items:?}");
}

#[test]
fn each_kind_of_event_the_chat_shows_is_a_line_of_the_right_role_and_tone() {
    assert_eq!(one("chat.say", json!({ "text": "hi" })), (Role::You, Tone::Plain, "hi".into()));
    assert_eq!(one("chat.note", json!({ "text": "n", "tone": "bad" })), (Role::Notice, Tone::Bad, "n".into()));
    assert_eq!(one("chat.note", json!({ "text": "n", "tone": "warn" })).1, Tone::Warn);
    assert_eq!(one("chat.note", json!({ "text": "n", "tone": "good" })).1, Tone::Good);
    assert_eq!(one("chat.note", json!({ "text": "n" })).1, Tone::Dim);
    assert_eq!(one("run.reasoning", json!({ "text": "hm" })), (Role::Reasoning, Tone::Dim, "hm".into()));
    assert_eq!(one("run.tool_call", json!({ "name": "Read", "detail": "a.rs" })), (Role::Tool, Tone::Plain, "Read a.rs".into()));
    assert_eq!(one("run.tool_result", json!({ "name": "Read", "detail": "ok" })), (Role::Tool, Tone::Dim, "Read → ok".into()));
    assert_eq!(one("run.notice", json!({ "text": "careful" })), (Role::Notice, Tone::Dim, "careful".into()));
    assert_eq!(one("run.error", json!({ "text": "boom" })), (Role::Error, Tone::Bad, "boom".into()));
    assert_eq!(one("run.changed", json!({ "paths": ["a", "b"] })), (Role::Notice, Tone::Dim, "changed a, b".into()));
    assert_eq!(one("task.cap_reached", json!({})).1, Tone::Warn);
    assert_eq!(one("thread.fresh", json!({})).2, "fresh: the next line chooses again");
    assert_eq!(one("thread.bound", json!({ "doer": "claude", "model": "haiku", "how": "pinned" })).2, "bound to claude · haiku (pinned)");
    assert_eq!(one("thread.bound", json!({ "doer": "test", "how": "elected" })).2, "bound to test (elected)");
}

#[test]
fn a_shell_line_shows_the_command_the_end_of_its_output_and_a_bad_exit() {
    assert_eq!(one("run.shell", json!({ "cmd": "make", "out": "", "exit": 0 })), (Role::Shell, Tone::Plain, "$ make".into()));
    let (role, tone, text) = one("run.shell", json!({ "cmd": "make", "out": "built\n", "exit": 3 }));
    assert_eq!((role, tone), (Role::Shell, Tone::Bad));
    assert_eq!(text, "$ make\nbuilt\nexit 3");
    let out: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let (_, _, text) = one("run.shell", json!({ "cmd": "x", "out": out, "exit": 0 }));
    assert!(text.contains("… 22 lines above\nline 23"), "{text}");
    assert!(text.ends_with("line 30"));
    let (_, _, text) = one("run.shell", json!({ "cmd": "x", "out": "y".repeat(500), "exit": 0 }));
    assert!(text.lines().nth(1).unwrap().chars().count() == 200 && text.contains('…'));
}

#[test]
fn a_task_that_stopped_says_why_in_words() {
    let why = |w: Value| one("task.stopped", json!({ "why": w }));
    assert_eq!(why(json!({ "kind": "aborted" })), (Role::Notice, Tone::Dim, "stopped".into()));
    assert_eq!(why(json!({ "kind": "wall" })), (Role::Notice, Tone::Warn, "stopped: the time it was given ran out".into()));
    assert_eq!(why(json!({ "kind": "hardcap" })), (Role::Notice, Tone::Bad, "stopped: it spent past the hard cap".into()));
    assert_eq!(why(json!({ "kind": "failed", "message": "it broke" })), (Role::Notice, Tone::Bad, "failed: it broke".into()));
    assert_eq!(why(json!({ "kind": "killed" })).2, "stopped: killed");
    assert_eq!(why(json!({ "kind": "from the future" })).2, "stopped");
    assert_eq!(one("task.stopped", json!({})).2, "stopped");
}

#[test]
fn a_task_that_finished_badly_says_why_and_one_that_finished_well_says_nothing_of_itself() {
    assert_eq!(one("task.settled", json!({ "ok": false, "reason": "it did nothing" })), (Role::Error, Tone::Bad, "it did nothing".into()));
    none("task.settled", json!({ "ok": false, "reason": "  " }));
    none("task.settled", json!({ "ok": false }));
    none("task.settled", json!({ "ok": true, "answer": "x" }));
}

#[test]
fn an_answer_to_a_question_and_the_gates_word_are_notes() {
    assert_eq!(one("ask.answered", json!({ "answer": "yes", "via": "human" })).2, "you: yes");
    assert_eq!(one("ask.answered", json!({ "answer": "no", "via": "chain", "by": "rule:gate", "why": "pushing is yours" })).2, "rule:gate: no — pushing is yours");
    assert_eq!(one("gate.verdict", json!({ "verdict": "block", "verb": "shell", "target": "git push" })), (Role::Notice, Tone::Bad, "block shell git push".into()));
    assert_eq!(one("gate.verdict", json!({ "verdict": "allow", "verb": "write", "target": "a" })).1, Tone::Dim);
}

#[test]
fn what_the_chat_does_not_show_it_skips() {
    for kind in ["task.queued", "task.started", "task.waiting", "run.metrics", "run.session", "intake.decided", "ask.step", "grant.used", "timer.fired", "thing.unheard.of"] {
        none(kind, json!({ "text": "x", "doer": "d" }));
    }
}

#[test]
fn text_with_newlines_is_kept_for_the_screen_to_wrap_and_one_line_notes_are_cut_short() {
    assert_eq!(one("run.message", json!({ "text": "a\nb" })).2, "a\nb");
    let long = "word ".repeat(100);
    let (_, _, t) = one("run.tool_call", json!({ "name": "n", "detail": long }));
    assert_eq!(t.chars().count(), 200);
    assert!(t.ends_with('…'));
}

#[test]
fn a_task_that_streams_two_blocks_gets_two_lines_and_two_tasks_stream_side_by_side() {
    let mut chat = Chat::new();
    let delta = |task: &str, t: &str| Ev { seq: 0, task: Some(task.into()), ..ev(0, "run.delta", json!({ "text": t })) };
    let key = |items: &[Item]| match &items[0] {
        Item::Put { line } => line.key.clone(),
        Item::Append { key, .. } => key.clone(),
        Item::Drop { key } => key.clone(),
    };
    let a1 = key(&chat.items(&delta("t1", "a")));
    let b1 = key(&chat.items(&delta("t2", "b")));
    assert_ne!(a1, b1);
    assert_eq!(key(&chat.items(&delta("t1", "a2"))), a1);
    let done = chat.items(&ev(5, "run.message", json!({ "text": "A" })));
    assert_eq!(key(&done), a1);
    let a2 = key(&chat.items(&delta("t1", "next")));
    assert_ne!(a2, a1);
    assert_eq!(key(&chat.items(&delta("t2", "b2"))), b1);
}

#[test]
fn what_the_rules_do_around_a_task_is_a_line_in_the_chat() {
    assert_eq!(one("task.cooling", json!({ "ms": 1_800_000, "until_ms": 5, "why": "cool-on-limit" })), (Role::Notice, Tone::Warn, "a limit was hit: it goes on by itself in 30m00s".into()));
    assert_eq!(one("task.compacting", json!({ "keep": "…" })), (Role::Notice, Tone::Dim, "shrinking the session before it goes on".into()));
    assert_eq!(one("task.compacted", json!({ "ok": true, "note": "900 → 120 tokens" })), (Role::Notice, Tone::Dim, "the session was shrunk (900 → 120 tokens)".into()));
    assert_eq!(one("task.compacted", json!({ "ok": true })).2, "the session was shrunk");
    assert_eq!(one("task.compacted", json!({ "ok": false, "note": null })), (Role::Notice, Tone::Warn, "the session was not shrunk".into()));
    // `:compact` says the sizes: what it held, what it holds, what was cut; and when the doer did not say the second, that it shows with the next line
    assert_eq!(one("task.compacting", json!({ "keep": "…", "only": true })), (Role::Notice, Tone::Dim, "shrinking the session".into()));
    assert_eq!(one("task.compacted", json!({ "ok": true, "held": 108_000, "held_after": 31_000, "cut": 77_000 })).2, "the session was shrunk: 108k → 31k (cut 77k)");
    assert_eq!(one("task.compacted", json!({ "ok": true, "held": 108_000 })).2, "the session was shrunk: 108k held before; what it holds now shows with the next line");
    let work = json!({ "files": ["src/a.rs", "b.rs"], "ignored": ["ledger/x.json"], "added": 4, "removed": 1 });
    assert_eq!(
        one("task.kept", json!({ "dir": "/h/worktrees/k/t5", "work": work, "why": "waiting to be applied" })),
        (Role::Notice, Tone::Warn, "kept in /h/worktrees/k/t5, not in your folder: 2 files +4 −1 · 1 ignored by git (src/a.rs, b.rs, ledger/x.json) — waiting to be applied".into())
    );
    assert_eq!(one("task.applied", json!({ "work": work, "notes": [] })), (Role::Notice, Tone::Accent, "laid into your folder, uncommitted: 2 files +4 −1 · 1 ignored by git (src/a.rs, b.rs, ledger/x.json)".into()));
    assert_eq!(one("task.applied", json!({ "work": work, "notes": ["no checkpoint, so this cannot be rewound: too many files"] })).2, "laid into your folder, uncommitted: 2 files +4 −1 · 1 ignored by git (src/a.rs, b.rs, ledger/x.json) (no checkpoint, so this cannot be rewound: too many files)");
    let many = json!({ "files": ["1", "2", "3", "4", "5", "6", "7"], "ignored": [], "added": 0, "removed": 0 });
    assert_eq!(one("task.applied", json!({ "work": many })).2, "laid into your folder, uncommitted: 7 files +0 −0 (1, 2, 3, 4, 5, +2 more)");
    assert_eq!(one("task.applied", json!({})).2, "laid into your folder, uncommitted: what it changed");
    assert_eq!(one("rule.dropped", json!({ "rule": "nag", "why": "it is over" })), (Role::Notice, Tone::Warn, "rule nag did not act: it is over".into()));
    let (role, tone, text) = one("task.stopped", json!({ "why": { "kind": "capped" } }));
    assert_eq!((role, tone), (Role::Notice, Tone::Warn));
    assert!(text.contains("soft cap") && text.contains("carry on"), "{text}");
    // a task a rule began says which rule and what it was told; one the operator began says nothing
    let queued = |by: Actor| Ev { by, ..ev(7, "task.queued", json!({ "spec": { "goal": "2 item(s) are still open." } })) };
    let items = Chat::new().items(&queued(Actor::Rule("followups".into())));
    let Item::Put { line } = &items[0] else { panic!("{items:?}") };
    assert_eq!((line.role, line.tone, line.text.as_str()), (Role::Notice, Tone::Accent, "carried on by rule:followups: 2 item(s) are still open."));
    assert!(Chat::new().items(&queued(Actor::Operator)).is_empty());
}

#[test]
fn a_binding_remembers_the_machine_pick_that_made_it_and_counts_what_came_after() {
    use j4n_core::front::thread::{AnswerRef, OVERRIDE_LINES};
    let mut binds = Binds::new();
    let at = |kind: &str, thread: &str, data: serde_json::Value| j4n_core::log::Ev { seq: 1, at_ms: 1, kind: kind.into(), by: j4n_core::change::Actor::Core, task: None, thread: Some(thread.into()), cause: None, class: j4n_core::schema::DataClass::Project, data };
    let ask = serde_json::json!({ "ask": "who:work#1-2", "answerer": "decider:jev", "model": "jev@1", "point": "who:work" });
    binds.apply(&at("thread.bound", "a", serde_json::json!({ "doer": "claude", "how": "elected", "endpoint": "Cerebras", "answer": ask })));
    binds.apply(&at("thread.bound", "b", serde_json::json!({ "doer": "pi", "how": "pinned" })));
    let b = binds.get("a").unwrap();
    assert_eq!((b.endpoint.as_deref(), b.lines, b.tasks), (Some("Cerebras"), 0, 0));
    assert_eq!(b.answer, Some(AnswerRef { ask: "who:work#1-2".into(), answerer: "decider:jev".into(), model: "jev@1".into(), point: "who:work".into() }));
    assert!(b.recent_answer().is_some());
    assert!(binds.get("b").unwrap().answer.is_none());
    // lines of the operator and tasks a doer began count; a question put to someone does not
    for _ in 0..OVERRIDE_LINES + 1 {
        binds.apply(&at("chat.say", "a", serde_json::json!({ "text": "x" })));
    }
    binds.apply(&at("task.started", "a", serde_json::json!({ "doer": "claude" })));
    binds.apply(&at("task.started", "a", serde_json::json!({ "doer": "human" })));
    binds.apply(&at("chat.say", "nobody", serde_json::json!({})));
    let b = binds.get("a").unwrap();
    assert_eq!((b.lines, b.tasks), (OVERRIDE_LINES + 1, 1));
    assert!(b.recent_answer().is_none(), "three lines later it is no longer recent");
    let read = |path: &[&str]| j4n_core::fold::Fold::read(&binds, &path.iter().map(|k| j4n_core::value::Key::Name((*k).into())).collect::<Vec<_>>(), 0);
    assert_eq!(read(&["a", "picked", "answerer"]), j4n_core::value::Value::text("decider:jev"), "the rules read it whatever the lines");
    assert_eq!(read(&["a", "tasks"]), j4n_core::value::Value::num(1.0));
    assert_eq!(read(&["a", "lines"]), j4n_core::value::Value::num(3.0));
    assert_eq!(read(&["a", "endpoint"]), j4n_core::value::Value::text("Cerebras"));
    assert_eq!(read(&["b", "picked"]), j4n_core::value::Value::Missing);
    // a restart finds the same
    let mut loaded = Binds::new();
    loaded.load(binds.save()).unwrap();
    assert_eq!(loaded.get("a"), binds.get("a"));
    // :fresh forgets it
    binds.apply(&at("thread.fresh", "a", serde_json::json!({})));
    assert_eq!(binds.get("a"), None);
}

#[test]
fn the_host_a_breaker_and_a_failed_probe_are_lines_in_the_chat() {
    assert_eq!(one("thread.bound", json!({ "doer": "or", "model": "vendor/m", "endpoint": "Cerebras", "how": "elected" })), (Role::Notice, Tone::Accent, "bound to or · vendor/m on Cerebras (elected)".into()));
    assert_eq!(one("thread.bound", json!({ "doer": "claude", "how": "pinned" })).2, "bound to claude (pinned)");
    assert_eq!(one("answerer.off", json!({ "answerer": "decider:jev", "failures": 3, "until_ms": 9 })), (Role::Notice, Tone::Warn, "decider:jev failed 3 times in a row: it is not asked again for a day".into()));
    assert_eq!(
        one("probe.result", json!({ "answerer": "decider:jev", "point": "intake", "passed": false, "failed": ["a", "b"], "model": "m" })),
        (Role::Notice, Tone::Warn, "decider:jev did not pass its cases for intake (wrong on 2): it does not answer there until it does".into())
    );
    assert_eq!(one("probe.result", json!({ "answerer": "model:x", "point": "pick", "passed": false, "why": "egress to `far` is blocked: never" })).2, "model:x did not pass its cases for pick (egress to `far` is blocked: never): it does not answer there until it does");
    let items = Chat::new().items(&ev(1, "probe.result", json!({ "answerer": "a", "point": "p", "passed": true })));
    assert!(items.is_empty(), "a pass is not worth a line");
}

// ---- the phrases that start a flow ----

#[test]
fn a_phrase_may_start_a_flow_with_a_slash_and_the_rest_of_the_line_is_its_goal() {
    let (p, found) = table("[[phrase]]\nid = \"plan\"\nwords = [\"/plan\", \":plan\"]\nflow = \"plan\"\n[[phrase]]\nid = \"fresh\"\nwords = [\":fresh\"]\ncommand = \"fresh\"\n");
    assert!(found.is_empty(), "{found:?}");
    let cmd = |line: &str| p.parse(line).map(|c| (c.name, c.args));
    assert_eq!(cmd("/plan add a flag"), Some(("flow".into(), vec!["plan".into(), "add".into(), "a".into(), "flag".into()])));
    assert_eq!(cmd("/PLAN x"), Some(("flow".into(), vec!["plan".into(), "x".into()])));
    assert_eq!(cmd(":plan"), Some(("flow".into(), vec!["plan".into()])));
    assert_eq!(cmd(":fresh"), Some(("fresh".into(), vec![])));
    assert_eq!(p.flows(), ["plan"]);
    // not a word of its own unless it opens the line, and a slash is not a command's
    assert_eq!(p.parse("please /plan it"), None);
}

#[test]
fn a_phrase_runs_a_command_or_starts_a_flow_not_both_and_not_neither_and_a_command_keeps_its_colon() {
    for (rows, says) in [
        ("[[phrase]]\nid = \"a\"\nwords = [\":a\"]\ncommand = \"fresh\"\nflow = \"plan\"\n", "one of the two"),
        ("[[phrase]]\nid = \"a\"\nwords = [\":a\"]\n", "one of the two"),
        ("[[phrase]]\nid = \"a\"\nwords = [\"/fresh\"]\ncommand = \"fresh\"\n", "opens with a colon"),
        ("[[phrase]]\nid = \"a\"\nwords = [\"plan\"]\nflow = \"plan\"\n", "colon or a slash"),
        ("[[phrase]]\nid = \"a\"\nwords = [\"/\"]\nflow = \"plan\"\n", "colon or a slash"),
    ] {
        let (_, found) = table(rows);
        assert!(found.iter().any(|f| f.contains(says)), "{rows}: {found:?}");
    }
}

/// The fold `threads` as it was at version 4: its state and its events are the same, and it does not know the chats.
struct Old(Binds);

impl Fold for Old {
    fn id(&self) -> &str {
        "threads"
    }
    fn version(&self) -> u64 {
        4
    }
    fn wants(&self, kind: &str) -> bool {
        self.0.wants(kind)
    }
    fn apply(&mut self, ev: &Ev) {
        self.0.apply(ev)
    }
    fn read(&self, path: &[j4n_core::value::Key], now_ms: u64) -> j4n_core::value::Value {
        self.0.read(path, now_ms)
    }
    fn save(&self) -> Value {
        self.0.save()
    }
    fn load(&mut self, state: Value) -> Result<(), String> {
        self.0.load(state)
    }
    fn reset(&mut self) {
        self.0.reset()
    }
}

#[test]
fn a_log_written_before_the_chats_is_refolded_with_the_new_fold_into_no_chats_and_the_same_bindings() {
    let s = Scratch::new("binds-old");
    let (log, _) = open_log(&s);
    let dir = s.join(".j4n/folds");
    let old = Folds::new(&dir);
    old.attach(&log).unwrap();
    let before = Binds::new();
    assert!(old.add(Box::new(Old(before.clone())), log.head()).is_empty());
    log.append(bound("main", "claude", Some("haiku"), "pinned")).unwrap();
    log.append(bound("b", "test", None, "elected")).unwrap();
    for n in 1..=2 {
        log.append(New::of("task.queued", Actor::Operator, json!({})).task(&format!("t{n}")).thread("main")).unwrap();
        log.append(New::of("chat.say", Actor::Operator, json!({ "text": "fix parser.rs" })).thread("main")).unwrap();
        log.append(New::of("task.started", Actor::Core, json!({ "doer": "claude" })).task(&format!("t{n}")).thread("main")).unwrap();
    }
    log.append(New::of("thread.fresh", Actor::Operator, json!({})).thread("b")).unwrap();
    assert!(before.chats().is_empty() && before.get("main").is_some());
    assert!(old.snapshot().is_empty());
    log.sync();
    // the next run has the fold of version 8: the snapshot of version 4 is not used, and the Log is read again
    let host = Folds::new(&dir);
    host.attach(&log).unwrap();
    let after = Binds::new();
    assert!(host.add(Box::new(after.clone()), log.head()).is_empty());
    assert!(host.catch_up(&log).is_empty());
    assert!(after.chats().is_empty() && after.mail().is_empty() && after.route_open().is_empty());
    for t in ["main", "b"] {
        assert_eq!(after.get(t), before.get(t), "{t}");
        assert_eq!(after.cut(t), before.cut(t), "{t}");
    }
    assert_eq!(after.get("main").map(|b| (b.doer, b.lines, b.tasks)), Some(("claude".into(), 2, 2)));
    assert_eq!(after.get("b"), None);
    assert_eq!(host.applied("threads"), Some(log.head()));
}
