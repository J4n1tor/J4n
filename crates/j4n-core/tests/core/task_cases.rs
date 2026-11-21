//! The loop through its public API, over scripted doers: what a task does from queued to over, what it writes to the Log on
//! the way, and the floors the loop keeps whatever the doer says (law 9).

use crate::scripted::{Beat, Rig, Scripted, approval, choice, end, nothing, pick, say};
use j4n_core::ask::{Answer, AskShape, AskSpec};
use j4n_core::change::Actor;
use j4n_core::doer::{Caps, RunCtl};
use j4n_core::fold::Fold;
use j4n_core::task::{Book, Ctl, Mode, State, TaskSpec, Tasks, Tune, Why};
use j4n_core::value::Key;
use serde_json::json;

fn why(rig: &Rig, id: &str) -> Why {
    match rig.finished(id).state {
        State::Stopped { why } => why,
        other => panic!("{id} is {other:?}\n{}", rig.dump()),
    }
}

fn failed(rig: &Rig, id: &str) -> String {
    match why(rig, id) {
        Why::Failed { message } => message,
        other => panic!("{other:?}"),
    }
}

fn waiting_on(rig: &Rig, id: &str) -> String {
    match rig.until(id, "waited", |r| matches!(r.state, State::Waiting { .. })).state {
        State::Waiting { ask } => ask,
        other => panic!("{other:?}"),
    }
}

fn spawn_with(rig: &Rig, doer: &str, f: impl FnOnce(&mut TaskSpec)) -> String {
    let mut spec = TaskSpec::new("go", doer);
    f(&mut spec);
    rig.tasks.spawn(spec, Actor::Operator).expect("spawns")
}

// ---- Tier 0 ----

#[test]
fn a_doer_that_ends_with_no_answer_and_no_sign_of_work_did_not_do_it() {
    let rig = Rig::new(vec![Scripted::new("quiet", vec![vec![nothing()]])]);
    let id = rig.spawn("write the report", "quiet");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: false });
    assert!(row.reason.as_deref().is_some_and(|r| r.contains("no answer and no sign of work")), "{:?}", row.reason);
    assert_eq!(row.answer, None);
    let settled = rig.of_kind("task.settled");
    assert_eq!((settled[0].data["ok"].clone(), settled[0].data["reason"].is_string()), (json!(false), true));
}

#[test]
fn a_task_that_said_or_did_something_is_not_a_task_that_did_nothing() {
    let turns = vec![
        vec![Beat::Tool("read".into(), "a.txt".into()), nothing()],
        vec![Beat::Shell("true".into(), String::new(), 0), nothing()],
        vec![Beat::Changed(vec!["a.txt".into()]), nothing()],
        vec![say("hello"), nothing()],
        vec![Beat::Delta("hello".into()), nothing()],
        vec![end("hello")],
        vec![Beat::Put("made.txt".into(), "x".into()), nothing()],
        vec![Beat::Notice("thinking".into()), Beat::Think("hm".into()), nothing()],
        vec![Beat::Reasoned("hm".into()), end("  ")],
    ];
    let expect = [true, true, true, true, true, true, true, false, false];
    let rig = Rig::new(vec![Scripted::new("d", turns)]);
    for (n, ok) in expect.into_iter().enumerate() {
        let id = rig.spawn("go", "d");
        let row = rig.finished(&id);
        assert_eq!(row.state, State::Done { ok }, "script {n}: {:?}", row.reason);
        assert_eq!(row.reason.is_some(), !ok);
    }
}

#[test]
fn a_mutating_task_in_a_folder_with_no_git_repo_runs_in_place() {
    let doer = Scripted::new("script", vec![vec![Beat::Put("notes/a.txt".into(), "hello\n".into()), end("saved")]]);
    let rig = Rig::new(vec![doer.clone()]);
    rig.project.put("keep.txt", "mine\n");
    let id = rig.spawn("take notes", "script");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!(doer.starts()[0].dir, rig.project.path(), "the doer works in the folder itself, not in a copy");
    assert_eq!(doer.starts()[0].task, id);
    assert_eq!(rig.project.read("notes/a.txt"), "hello\n");
    assert!(!rig.project.exists(".git"), "no repository was made to hold the work");
    assert_eq!(row.changed.created, ["notes/a.txt"], "what changed is said");
    assert!(rig.of_kind("run.notice").is_empty(), "there was a checkpoint, so there is nothing to say");
    assert!(rig.home.exists("checkpoints"), "the checkpoint is filed in the home");
}

#[test]
fn rewind_restores_a_tracked_edit_deletes_a_file_the_task_created_and_restores_an_ignored_file() {
    let rig = Rig::new(vec![Scripted::new(
        "script",
        vec![vec![
            Beat::Put("a.txt".into(), "two\n".into()),
            Beat::Remove("gone.txt".into()),
            Beat::Put("ledger/found/x.json".into(), "{}\n".into()),
            Beat::Put("ledger/old.json".into(), "{\"v\":2}\n".into()),
            Beat::Put("out/reports/s.md".into(), "hi\n".into()),
            end("done"),
        ]],
    )]);
    let root = rig.project.path();
    let run = |args: &[&str]| {
        let st = std::process::Command::new("git").args(args).current_dir(root).status().unwrap();
        assert!(st.success(), "{args:?}");
    };
    run(&["init", "-q", "."]);
    rig.project.put(".gitignore", "ledger/\n");
    rig.project.put("a.txt", "one\n");
    rig.project.put("gone.txt", "bye\n");
    rig.project.put("ledger/old.json", "{\"v\":1}\n");
    run(&["add", "-A"]);
    run(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"]);
    let id = rig.spawn("change things", "script");
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!((row.changed.created.len(), row.changed.modified.len(), row.changed.deleted.len()), (2, 2, 1), "{:?}", row.changed);
    assert_eq!(rig.project.read("a.txt"), "two\n");
    assert!(!rig.project.exists("gone.txt"));
    let report = rig.tasks.rewind(None, false).unwrap();
    assert_eq!(report.task, id);
    assert_eq!((report.restored.len(), report.removed.len()), (3, 2), "{report:?}");
    assert_eq!(rig.project.read("a.txt"), "one\n", "a tracked edit is back");
    assert_eq!(rig.project.read("gone.txt"), "bye\n", "a deleted file is back");
    assert_eq!(rig.project.read("ledger/old.json"), "{\"v\":1}\n", "an ignored file is back");
    assert!(!rig.project.exists("ledger/found/x.json") && !rig.project.exists("out"), "what the task created is gone");
    let rewound = rig.of_kind("task.rewound");
    assert_eq!((rewound.len(), rewound[0].task.as_deref(), &rewound[0].by), (1, Some(id.as_str()), &Actor::Operator));
    assert_eq!(rewound[0].data["restored"], 3);
}

#[test]
fn rewind_waits_for_a_task_that_is_still_going_and_for_one_that_has_a_checkpoint() {
    let doer = Scripted::new("script", vec![vec![Beat::Put("a.txt".into(), "x".into()), Beat::Forever]]);
    let rig = Rig::new(vec![doer]);
    let id = rig.spawn("go", "script");
    rig.until(&id, "started", |r| r.state == State::Running);
    let err = rig.tasks.rewind(Some(&id), false).unwrap_err();
    assert!(err.msg.contains("still going"), "{err}");
    assert!(rig.tasks.rewind(Some("t77"), false).unwrap_err().msg.contains("no checkpoint"));
    assert!(rig.tasks.rewind(Some("../x"), false).unwrap_err().msg.contains("is not a task"));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    rig.finished(&id);
    rig.tasks.rewind(Some(&id), false).unwrap();
}

#[test]
fn a_read_task_is_not_checkpointed_and_a_checkpoint_that_cannot_be_made_is_said() {
    let doer = Scripted::new("script", vec![vec![end("looked")]]);
    let rig = Rig::new(vec![doer]);
    let id = spawn_with(&rig, "script", |s| s.tune.mode = Mode::Read);
    rig.finished(&id);
    assert!(!rig.home.exists("checkpoints"), "a task that cannot write has nothing to rewind");
    // a folder too big to walk: the task asks before it starts; a yes goes on and says it cannot be rewound, a no stops it
    let big = Rig::new(vec![Scripted::new("script", vec![vec![end("done")]]), Scripted::new("second", vec![vec![end("done")]])]);
    let lots = big.project.join("lots");
    std::fs::create_dir_all(&lots).unwrap();
    for i in 0..100_005 {
        std::fs::write(lots.join(format!("f{i}")), "").unwrap();
    }
    let id = big.spawn("go", "script");
    let ask = waiting_on(&big, &id);
    let q = big.tasks.book().get(&ask).unwrap().spec.ask().unwrap();
    assert!(q.question.contains("no checkpoint") && q.question.contains("Go on with no checkpoint, or stop?"), "{}", q.question);
    assert!(matches!(q.shape, AskShape::Approval { .. }) && q.act.is_none());
    big.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    assert_eq!(big.finished(&id).state, State::Done { ok: true });
    let notices = big.of_kind("run.notice");
    assert!(notices.len() == 1 && notices[0].data["text"].as_str().unwrap().contains("cannot be rewound"), "{notices:?}");
    assert!(big.tasks.rewind(Some(&id), false).is_err());
    // a no: the doer never starts
    let id = big.spawn("go again", "second");
    let ask = waiting_on(&big, &id);
    big.tasks.ctl(&ask, Ctl::Answer(Answer::No), Actor::Operator).unwrap();
    assert_eq!(why(&big, &id), Why::Aborted);
    assert!(big.of_kind("run.notice").iter().any(|n| n.data["text"].as_str().unwrap().contains("stopped before it started")));
    assert!(big.of_kind("run.message").iter().all(|m| m.task.as_deref() != Some(id.as_str())), "the second doer never said anything");
}

// ---- what is written, and when ----

#[test]
fn a_task_writes_what_happens_to_it_in_order() {
    let doer = Scripted::new(
        "script",
        vec![vec![
            Beat::Session("s1".into()),
            Beat::Delta("let me look ".into()),
            Beat::Think("the file is small".into()),
            Beat::Tool("read".into(), "a.txt".into()),
            Beat::ToolEnd("read".into(), "12 bytes".into()),
            Beat::Reasoned("so it is fine".into()),
            Beat::Shell("ls".into(), "a.txt\n".into(), 3),
            Beat::Spend(0.001),
            say("all fine"),
            Beat::End(Some("all fine".into()), 1),
        ]],
    );
    let rig = Rig::new(vec![doer]);
    let mut spec = TaskSpec::new("look", "script");
    spec.thread = Some("chat-1".into());
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!((row.state, row.todo_left, row.spent), (State::Done { ok: true }, 1, 1_000));
    let kinds = rig.kinds(&id);
    assert_eq!(
        kinds,
        [
            "task.queued",
            "task.started",
            "run.session",
            "run.message", // "let me look " kept as narration when the tool call came
            "run.tool_call",
            "run.tool_result",
            "run.reasoning",
            "run.shell",
            "run.metrics",
            "run.message",
            "task.settled",
        ]
    );
    let evs: Vec<_> = rig.events().into_iter().filter(|e| e.task.as_deref() == Some(id.as_str())).collect();
    assert!(evs.iter().all(|e| e.thread.as_deref() == Some("chat-1")), "every event of the task carries its thread");
    assert_eq!(evs[0].by, Actor::Operator);
    assert_eq!(evs[1].by, Actor::Core);
    assert_eq!(evs[3].by, Actor::Doer("script".into()));
    assert_eq!((evs[3].data["narration"].clone(), evs[3].data["text"].clone()), (json!(true), json!("let me look ")));
    assert_eq!(evs[7].data, json!({ "cmd": "ls", "out": "a.txt\n", "exit": 3 }), "the real exit code is persisted");
    assert_eq!(evs[10].data["todo_left"], 1);
    assert!(rig.of_kind("run.reasoning_delta").is_empty() && rig.of_kind("run.delta").is_empty(), "deltas are flashed, never written");
}

#[test]
fn deltas_are_flashed_to_whoever_listens_and_to_nobody_else() {
    let rig = Rig::new(vec![Scripted::new("script", vec![vec![Beat::Delta("he".into()), Beat::Think("hm".into()), Beat::Delta("llo".into()), end("hello")]])]);
    let sub = rig.log.subscribe("run.*", 100);
    let id = rig.spawn("greet", "script");
    rig.finished(&id);
    let mut flashes = Vec::new();
    while let Some(e) = sub.try_recv() {
        if e.seq == 0 {
            flashes.push((e.kind.clone(), e.data["text"].as_str().unwrap().to_string(), e.task.clone()));
        }
    }
    let flash = |k: &str, t: &str| (k.to_string(), t.to_string(), Some(id.clone()));
    assert_eq!(flashes, [flash("run.delta", "he"), flash("run.reasoning_delta", "hm"), flash("run.delta", "llo")]);
}

#[test]
fn the_answer_is_the_report_that_settled_last() {
    // narration before a tool call keeps its place; the report after it is the answer
    let doer = Scripted::new(
        "script",
        vec![
            vec![Beat::Delta("No project code exists here. I'll write a script. ".into()), Beat::Tool("write".into(), "s.py".into()), Beat::ToolEnd("write".into(), "ok".into()), Beat::Delta("Done: saved s.py.".into()), nothing()],
            vec![say("first I will look"), Beat::Tool("read".into(), "a".into()), say("the report"), nothing()],
            vec![say("I looked"), Beat::Tool("read".into(), "a".into()), end("the final report")],
            vec![say("same words"), end("same words")],
            vec![Beat::Delta("streamed words".into()), end("given words")],
            vec![say("the report"), Beat::Shell("true".into(), String::new(), 0), nothing()],
        ],
    );
    let rig = Rig::new(vec![doer]);
    let answers: Vec<Option<String>> = (0..6)
        .map(|_| {
            let id = rig.spawn("go", "script");
            rig.finished(&id).answer
        })
        .collect();
    let want = ["Done: saved s.py.", "the report", "the final report", "same words", "given words", ""];
    for (n, (got, want)) in answers.iter().zip(want).enumerate() {
        assert_eq!(got.as_deref().unwrap_or(""), want, "script {n}");
    }
    let narration: Vec<String> = rig
        .of_kind("run.message")
        .iter()
        .filter(|e| e.task.as_deref() == Some("t1") && e.data["narration"] == json!(true))
        .map(|e| e.data["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(narration, ["No project code exists here. I'll write a script. "]);
    let said = |t: &str| rig.of_kind("run.message").iter().filter(|e| e.data["text"] == json!(t)).count();
    assert_eq!(said("same words"), 1, "an answer that was already said is not said twice");
    assert_eq!(said("Done: saved s.py."), 1);
    assert_eq!(said("the final report"), 1);
    assert_eq!(said("streamed words"), 1, "the streamed words the answer replaced are kept as they were said");
}

#[test]
fn what_a_shell_command_printed_is_kept_by_its_tail() {
    let long = format!("{}{}", "a".repeat(100_000), "THE END\n");
    let rig = Rig::new(vec![Scripted::new("script", vec![vec![Beat::Shell("make".into(), long, 2), end("failed")]])]);
    let id = rig.spawn("build", "script");
    rig.finished(&id);
    let out = rig.of_kind("run.shell")[0].data["out"].as_str().unwrap().to_string();
    assert!(out.len() < 33_000 && out.starts_with('…') && out.ends_with("THE END\n"), "{}", out.len());
}

// ---- floors ----

#[test]
fn a_doer_nobody_registered_or_a_place_that_does_not_exist_stops_the_task_saying_which() {
    let rig = Rig::new(vec![Scripted::new("known", vec![vec![end("x")]])]);
    let a = rig.spawn("go", "stranger");
    assert_eq!(failed(&rig, &a), "no doer called `stranger`");
    let b = spawn_with(&rig, "known", |s| s.place = "moon".into());
    assert_eq!(failed(&rig, &b), "no place called `moon`");
    assert_eq!(rig.tasks.menu(&a).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["retry", "abort"]);
}

#[test]
fn a_spec_that_is_refused_makes_no_task_and_no_number_is_used() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![end("x")]])]);
    let err = rig.tasks.spawn(TaskSpec::new("   ", "d"), Actor::Operator).unwrap_err();
    assert!(err.msg.contains("goal"), "{err}");
    let mut orphan = TaskSpec::new("go", "d");
    orphan.parent = Some("t55".into());
    assert!(rig.tasks.spawn(orphan, Actor::Operator).unwrap_err().msg.contains("no task t55"));
    let mut lost = TaskSpec::new("go", "d");
    lost.follows = Some("t56".into());
    assert!(rig.tasks.spawn(lost, Actor::Operator).unwrap_err().msg.contains("carry on from"));
    assert!(rig.events().iter().all(|e| e.kind != "task.queued"));
    assert_eq!(rig.spawn("go", "d"), "t1");
    // a part of a task that is over is refused
    let mut late = TaskSpec::new("part", "d");
    late.parent = Some("t1".into());
    rig.finished("t1");
    assert!(rig.tasks.spawn(late, Actor::Operator).unwrap_err().msg.contains("is over"));
}

#[test]
fn spend_adds_up_and_the_soft_cap_says_so_once_and_lets_the_task_go_on() {
    let doer = Scripted::new("d", vec![vec![Beat::Spend(0.006), Beat::Spend(0.006), Beat::Spend(0.001), Beat::Idle, Beat::Spend(0.0), say("fine"), end("fine")]]);
    let rig = Rig::new(vec![doer]);
    let id = spawn_with(&rig, "d", |s| s.limits.usd_soft = Some(0.01));
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Done { ok: true });
    assert_eq!((row.spent, row.capped, row.tokens_in, row.tokens_out), (13_000, true, 40, 20), "tokens count when no money was spent");
    let caps = rig.of_kind("task.cap_reached");
    assert_eq!(caps.len(), 1);
    assert_eq!((caps[0].data["soft"].clone(), caps[0].data["spent"].clone()), (json!(0.01), json!(0.012)));
    assert_eq!(rig.of_kind("run.metrics").len(), 4, "a report that spent nothing at all is not an event");
}

#[test]
fn past_one_and_a_half_times_the_cap_the_task_is_killed_and_keeps_its_report() {
    let doer = Scripted::new("d", vec![vec![say("so far so good"), Beat::Spend(0.006), Beat::Spend(0.006), Beat::Spend(0.0031), say("never"), end("never")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = spawn_with(&rig, "d", |s| s.limits.usd_soft = Some(0.01));
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::HardCap });
    assert_eq!(row.answer.as_deref(), Some("so far so good"));
    assert_eq!(doer.ctls(), vec![RunCtl::Kill], "the hard cap kills");
    assert!(rig.of_kind("run.message").iter().all(|e| e.data["text"] != json!("never")));
    // exactly at one and a half times is not past it
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Spend(0.015), end("ok")]])]);
    let id = spawn_with(&rig, "d", |s| s.limits.usd_soft = Some(0.01));
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
}

#[test]
fn the_wall_stops_the_task_itself_with_its_report() {
    let doer = Scripted::new("d", vec![vec![say("halfway there"), Beat::Delta("still working".into()), Beat::Forever]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = spawn_with(&rig, "d", |s| s.limits.wall_ms = Some(5_000));
    rig.until_event("the first words", |e| e.kind == "run.message");
    rig.clock.advance(4_999);
    rig.timers.poke();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(rig.tasks.book().get(&id).unwrap().state, State::Running, "not before the wall");
    rig.clock.advance(1);
    rig.timers.poke();
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::Wall });
    assert_eq!(row.answer.as_deref(), Some("halfway there"), "it stops with a report");
    assert_eq!(doer.ctls(), vec![RunCtl::Abort]);
    assert_eq!(rig.tasks.menu(&id).iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["resume", "abort"]);
    let said: Vec<_> = rig.of_kind("run.message").iter().map(|e| (e.data["text"].clone(), e.data["partial"].clone())).collect();
    assert_eq!(said, [(json!("halfway there"), json!(null)), (json!("still working"), json!(true))], "each said once, the last as it stood");
    let stopped = rig.of_kind("task.stopped");
    assert_eq!(stopped[0].by, Actor::Core);
    assert_eq!(stopped[0].data["why"], json!({ "kind": "wall" }));
}

#[test]
fn a_doer_that_takes_words_while_it_works_is_told_the_time_is_nearly_up_once_before_the_wall() {
    let doer = Scripted::new("d", vec![vec![say("half done"), Beat::Forever]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = spawn_with(&rig, "d", |s| s.limits.wall_ms = Some(600_000));
    rig.until_event("the first words", |e| e.kind == "run.message");
    rig.clock.advance(527_999);
    rig.timers.poke();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(doer.ctls(), vec![], "not before 88 % of the time has passed");
    rig.clock.advance(1);
    rig.timers.poke();
    let told = rig.until_event("the warning", |e| e.kind == "task.steered");
    assert_eq!(told.by, Actor::Core);
    let text = told.data["text"].as_str().unwrap();
    assert!(text.contains("nearly up") && text.contains("about 72 seconds") && text.contains("Write the files"), "{text}");
    assert_eq!(doer.ctls(), vec![RunCtl::Steer(text.to_string())], "it reaches the running doer and does not restart it");
    rig.clock.advance(72_000);
    rig.timers.poke();
    let row = rig.finished(&id);
    assert_eq!(row.state, State::Stopped { why: Why::Wall });
    assert_eq!(doer.ctls().len(), 2, "the warning once, then the stop");
    assert_eq!(rig.of_kind("task.steered").len(), 1);
    assert_eq!(rig.of_kind("task.stopped").len(), 1, "the wall stops it once");
    assert_eq!(rig.timers.pending(), 0);
}

#[test]
fn a_doer_that_cannot_take_words_while_it_works_is_not_stopped_to_be_told_and_a_short_wall_is_not_warned() {
    let slow = Scripted::with_caps("slow", Caps { live: false, resume: true, ..Caps::default() }, vec![vec![say("on it"), Beat::Forever]]);
    let live = Scripted::new("live", vec![vec![say("on it"), Beat::Forever]]);
    let rig = Rig::new(vec![slow.clone(), live.clone()]);
    let short = spawn_with(&rig, "live", |s| s.limits.wall_ms = Some(60_000));
    assert_eq!(rig.timers.pending(), 1, "a wall under the setting `wall.warn.min` has no warning");
    rig.clock.advance(60_000);
    rig.timers.poke();
    assert_eq!(rig.finished(&short).state, State::Stopped { why: Why::Wall });
    assert_eq!(live.ctls(), vec![RunCtl::Abort], "the stop and nothing before it");
    let long = spawn_with(&rig, "slow", |s| s.limits.wall_ms = Some(600_000));
    rig.until_event("the first words", |e| e.kind == "run.message");
    assert_eq!(rig.timers.pending(), 2, "the wall and the warning");
    rig.clock.advance(528_000);
    rig.timers.poke();
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(rig.timers.pending(), 1, "the warning came due");
    assert_eq!(rig.tasks.book().get(&long).unwrap().state, State::Running, "it was not stopped and started again to be told");
    assert_eq!(slow.ctls(), vec![]);
    assert!(rig.of_kind("task.steered").is_empty(), "{}", rig.dump());
}

#[test]
fn a_timer_is_given_back_when_its_task_ends_and_one_that_fires_late_is_ignored() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![end("quick")], vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")]])]);
    let a = spawn_with(&rig, "d", |s| s.limits.wall_ms = Some(60_000));
    rig.finished(&a);
    assert_eq!(rig.timers.pending(), 0, "the wall of a finished task is cancelled");
    let b = spawn_with(&rig, "d", |s| s.limits.wall_ms = Some(60_000));
    let ask = waiting_on(&rig, &b);
    assert_eq!(rig.timers.pending(), 2, "a wall and a deadline");
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::No), Actor::Operator).unwrap();
    rig.finished(&b);
    assert_eq!(rig.timers.pending(), 0, "an answered question's deadline is cancelled too");
    rig.clock.advance(10_000_000);
    rig.timers.poke();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(rig.tasks.book().get(&b).unwrap().state, State::Done { ok: true });
}

#[test]
fn a_doer_that_panics_or_goes_away_without_a_report_is_a_failure_not_a_finish() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![say("starting"), Beat::Panic], vec![say("starting"), Beat::Vanish]])]);
    let a = rig.spawn("go", "d");
    assert_eq!(failed(&rig, &a), "the doer stopped without a report");
    assert_eq!(rig.tasks.book().get(&a).unwrap().answer.as_deref(), Some("starting"));
    let b = rig.spawn("go", "d");
    assert_eq!(failed(&rig, &b), "the doer stopped without a report");
}

#[test]
fn what_a_stopped_run_still_says_is_not_heard() {
    let doer = Scripted::leaking("d", vec![vec![Beat::Forever]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("go", "d");
    rig.until(&id, "started", |r| r.state == State::Running);
    while doer.starts().is_empty() {
        std::thread::yield_now();
    }
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    rig.finished(&id);
    let emit = doer.emit();
    let before = rig.events().len();
    emit.send(j4n_core::doer::RunEv::Message { role: j4n_core::doer::Role::Assistant, text: "too late".into() });
    emit.send(j4n_core::doer::RunEv::End { answer: Some("too late".into()), todo_left: 0 });
    // the loop handles messages in order: a task made now is handled after the late ones
    let next = rig.tasks.spawn(TaskSpec::new("go", "nobody"), Actor::Operator).unwrap();
    rig.finished(&next);
    let late: Vec<_> = rig.events().into_iter().skip(before).filter(|e| e.task.as_deref() == Some(id.as_str())).collect();
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(rig.tasks.book().get(&id).unwrap().state, State::Stopped { why: Why::Aborted });
}

// ---- orders ----

#[test]
fn a_steer_reaches_a_doer_that_listens() {
    let doer = Scripted::new("d", vec![vec![Beat::Session("s".into()), Beat::Forever]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("go", "d");
    rig.until_event("the session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Steer("also keep the tests green".into()), Actor::Operator).unwrap();
    assert_eq!(doer.ctls(), vec![RunCtl::Steer("also keep the tests green".into())]);
    assert_eq!(doer.starts().len(), 1);
    let steered = rig.of_kind("task.steered");
    assert_eq!((steered[0].by.clone(), steered[0].data["text"].clone()), (Actor::Operator, json!("also keep the tests green")));
    for bad in ["", "   "] {
        assert!(rig.tasks.ctl(&id, Ctl::Steer(bad.into()), Actor::Operator).unwrap_err().msg.contains("words"));
    }
    assert_eq!(doer.ctls().len(), 1, "a refused steer reaches nobody");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

#[test]
fn a_steer_to_a_doer_that_cannot_hear_stops_its_run_and_starts_it_again_on_its_session() {
    let doer = Scripted::with_caps("d", Caps { live: false, resume: true, ..Caps::default() }, vec![vec![Beat::Session("s-1".into()), Beat::Forever], vec![say("with the steer"), end("with the steer")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("go", "d");
    rig.until_event("the session", |e| e.kind == "run.session");
    rig.tasks.ctl(&id, Ctl::Steer("use sqlite".into()), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("with the steer")));
    assert_eq!(doer.ctls(), vec![RunCtl::Abort]);
    let starts = doer.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!((starts[1].resume.as_deref(), starts[1].text.as_deref(), starts[1].goal.as_str()), (Some("s-1"), Some("use sqlite"), "go"));
    assert_eq!(rig.of_kind("task.steered")[0].data["restarted"], true);
    assert_eq!(rig.of_kind("task.started").len(), 2, "the second run is a start in the Log");
    assert_eq!(rig.of_kind("task.started")[1].data["resumed"], true);
    // without a session to carry on in, it is refused and the run is left alone
    let deaf = Scripted::with_caps("deaf", Caps { live: false, resume: true, ..Caps::default() }, vec![vec![Beat::Forever]]);
    let rig = Rig::new(vec![deaf.clone()]);
    let id = rig.spawn("go", "deaf");
    rig.until(&id, "started", |r| r.state == State::Running);
    let err = rig.tasks.ctl(&id, Ctl::Steer("x".into()), Actor::Operator).unwrap_err();
    assert!(err.msg.contains("cannot carry on in a session"), "{err}");
    assert!(deaf.ctls().is_empty() && rig.of_kind("task.steered").is_empty());
}

#[test]
fn a_task_that_waits_is_steered_by_answering_and_a_task_that_is_over_by_resuming() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")], vec![end("one")]])]);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    assert!(rig.tasks.ctl(&id, Ctl::Steer("hurry".into()), Actor::Operator).unwrap_err().msg.contains("answer its question first"));
    assert!(rig.tasks.ctl(&id, Ctl::Retune(Tune::default()), Actor::Operator).unwrap_err().msg.contains("only a running task"));
    assert!(rig.tasks.resume(&id, None, Actor::Operator).unwrap_err().msg.contains("still going"));
    let q = rig.tasks.ctl(&ask, Ctl::Steer("hurry".into()), Actor::Operator).unwrap_err();
    assert!(q.msg.contains("this is a question"), "{q}");
    assert!(rig.tasks.ctl(&ask, Ctl::Abort, Actor::Operator).unwrap_err().msg.contains("this is a question"));
    assert!(rig.tasks.resume(&ask, None, Actor::Operator).is_err());
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    rig.finished(&id);
    assert!(rig.tasks.resume(&ask, None, Actor::Operator).unwrap_err().msg.contains("answered, not resumed"), "an answered question is over, and is still not a task to carry on");
    assert!(rig.tasks.ctl("t99", Ctl::Abort, Actor::Operator).unwrap_err().msg.contains("no task"));
    assert!(rig.tasks.ctl(&id, Ctl::Kill, Actor::Operator).unwrap_err().msg.contains("carry on with resume"));
    // a finished task is carried on by a new one that follows it
    let next = rig.tasks.resume(&id, None, Actor::Operator).unwrap();
    let row = rig.finished(&next);
    assert_eq!((row.spec.follows.as_deref(), row.spec.goal.as_str(), row.state), (Some(id.as_str()), "go", State::Done { ok: true }));
    let blank = rig.tasks.resume(&id, Some("   "), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&blank).spec.goal, "go", "words that are blank are no words");
}

#[test]
fn a_retune_stops_the_run_and_starts_it_on_its_session_with_the_new_settings() {
    let doer = Scripted::new("d", vec![vec![Beat::Session("s-1".into()), Beat::Forever], vec![say("on the new model"), end("on the new model")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("go", "d");
    rig.until_event("the session", |e| e.kind == "run.session");
    let tune = Tune { model: Some("haiku".into()), endpoint: None, effort: Some("low".into()), mode: Mode::Read };
    rig.tasks.ctl(&id, Ctl::Retune(tune.clone()), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!((row.state, row.spec.tune.clone()), (State::Done { ok: true }, tune));
    assert_eq!(doer.ctls(), vec![RunCtl::Abort]);
    assert_eq!(doer.starts()[1].resume.as_deref(), Some("s-1"));
    assert_eq!(rig.of_kind("task.retuned")[0].by, Actor::Operator);
    // a doer that cannot carry on cannot be retuned
    let fixed = Scripted::with_caps("fixed", Caps { live: true, resume: false, ..Caps::default() }, vec![vec![Beat::Session("s".into()), Beat::Forever]]);
    let rig = Rig::new(vec![fixed.clone()]);
    let id = rig.spawn("go", "fixed");
    rig.until_event("the session", |e| e.kind == "run.session");
    assert!(rig.tasks.ctl(&id, Ctl::Retune(Tune::default()), Actor::Operator).unwrap_err().msg.contains("cannot carry on"));
    assert!(fixed.ctls().is_empty());
}

// ---- questions ----

#[test]
fn a_question_whose_doer_ends_its_run_is_answered_by_starting_it_again() {
    let mut q = choice("db", "which database?", &[("postgres", true), ("sqlite", false)]);
    q.inband = false;
    let doer = Scripted::new("d", vec![vec![Beat::Session("s-1".into()), Beat::Ask(q), nothing()], vec![say("sqlite it is"), end("sqlite it is")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("set it up", "d");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Answer(pick("sqlite")), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("sqlite it is")));
    let starts = doer.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!((starts[1].resume.as_deref(), starts[1].text.as_deref()), (Some("s-1"), Some("sqlite")));
    assert!(doer.ctls().is_empty(), "the answer is carried by the new run, and the old one is not told anything: {:?}", doer.ctls());
}

#[test]
fn a_question_that_was_not_in_band_and_whose_doer_has_no_session_stops_the_task_when_answered() {
    let mut q = choice("db", "which?", &[("a", false)]);
    q.inband = false;
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(q), nothing()]])]);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Answer(pick("a")), Actor::Operator).unwrap();
    assert!(failed(&rig, &id).contains("cannot carry on in a session"));
}

#[test]
fn a_question_that_stays_open_when_an_in_band_doer_ends_is_a_failure() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(approval("q", "make")), end("done anyway")]])]);
    let id = rig.spawn("go", "d");
    assert!(failed(&rig, &id).contains("ended while its question was open"));
}

#[test]
fn an_approval_is_answered_by_the_operator_or_a_rule_never_by_a_model() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(approval("q", "rm -rf build")), Beat::Hold, end("done")]])]);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    let err = rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Model("light".into())).unwrap_err();
    assert!(err.msg.contains("never by a model"), "{err}");
    assert!(matches!(rig.tasks.book().get(&id).unwrap().state, State::Waiting { .. }));
    assert!(rig.tasks.ctl(&ask, Ctl::Answer(Answer::Text { text: "sure".into() }), Actor::Operator).is_err(), "an approval takes yes or no");
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Rule("grant".into())).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    assert_eq!(rig.of_kind("task.settled")[0].by, Actor::Rule("grant".into()), "who answered is on the record");
    // a choice may be answered by a model, and the answer is kept as it was given
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(choice("q", "which?", &[("a", false), ("b", false)])), Beat::Hold, end("done")]])]);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    rig.tasks.ctl(&ask, Ctl::Answer(pick("b")), Actor::Model("light".into())).unwrap();
    rig.finished(&id);
    let q = rig.of_kind("task.settled").into_iter().find(|e| e.task.as_deref() == Some(ask.as_str())).unwrap();
    assert_eq!((q.data["answer"].clone(), q.data["given"].clone()), (json!("b"), json!({ "answer": "pick", "id": "b" })));
}

#[test]
fn a_question_that_cannot_be_shown_stops_the_task_that_asked_it() {
    let bad = |q: AskSpec| {
        let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(q), Beat::Hold, end("x")]])]);
        let id = rig.spawn("go", "d");
        failed(&rig, &id)
    };
    assert!(bad(AskSpec { local: "q".into(), question: String::new(), shape: AskShape::YesNo, inband: true, point: None, act: None }).contains("cannot be shown"));
    assert!(bad(choice("q", "which?", &[])).contains("cannot be shown"));
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Ask(approval("a", "x")), Beat::Ask(approval("b", "y")), Beat::Hold, end("x")]])]);
    let id = rig.spawn("go", "d");
    assert!(failed(&rig, &id).contains("second question"));
    let open: Vec<_> = rig.tasks.book().rows().into_iter().filter(|r| !r.state.is_terminal()).collect();
    assert!(open.is_empty(), "the question it left open went with it: {open:?}");
}

#[test]
fn a_late_answer_to_a_parked_question_carries_the_task_on_in_its_session() {
    let q = choice("db", "which database?", &[("postgres", true), ("sqlite", false)]);
    let doer = Scripted::new("d", vec![vec![Beat::Session("s-1".into()), Beat::Ask(q), Beat::Hold, say("never")], vec![say("sqlite"), end("sqlite")]]);
    let rig = Rig::tuned(vec![doer.clone()], |c| c.ask_wait_ms = 1_000);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    rig.clock.advance(1_000);
    rig.timers.poke();
    rig.finished(&id);
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Stopped { why: Why::Deadline });
    assert!(rig.tasks.ctl(&ask, Ctl::Answer(pick("mysql")), Actor::Operator).is_err(), "the answer is still checked");
    rig.tasks.ctl(&ask, Ctl::Answer(pick("sqlite")), Actor::Operator).unwrap();
    let next = rig.tasks.book().rows().into_iter().find(|r| r.spec.follows.as_deref() == Some(id.as_str())).expect("a task that follows");
    let row = rig.finished(&next.id);
    assert_eq!((row.state, row.answer.as_deref()), (State::Done { ok: true }, Some("sqlite")));
    let starts = doer.starts();
    assert_eq!((starts[1].resume.as_deref(), starts[1].goal.as_str()), (Some("s-1"), "sqlite"));
    // and only once: the question is not open for a second answer
    let again = rig.tasks.ctl(&ask, Ctl::Answer(pick("postgres")), Actor::Operator).unwrap_err();
    assert!(again.msg.contains("nobody is waiting"), "{again}");
    // a question that is not parked has nothing to say to a task that is over
    let done = Scripted::new("e", vec![vec![end("x")]]);
    let rig = Rig::new(vec![done]);
    let id = rig.spawn("go", "e");
    rig.finished(&id);
    assert!(rig.tasks.ctl(&id, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap_err().msg.contains("not a question"));
}

#[test]
fn a_question_is_a_task_of_the_task_that_asked_it() {
    let doer = Scripted::new("d", vec![vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")]]);
    let rig = Rig::new(vec![doer]);
    let mut spec = TaskSpec::new("go", "d");
    spec.thread = Some("chat-9".into());
    let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
    let ask = waiting_on(&rig, &id);
    let q = rig.tasks.book().get(&ask).unwrap();
    assert_eq!((q.spec.parent.as_deref(), q.spec.doer.as_str(), q.spec.thread.as_deref()), (Some(id.as_str()), "human", Some("chat-9")));
    assert_eq!(q.spec.ask().unwrap().local, "q");
    assert_eq!(rig.tasks.book().children(&id).len(), 1);
    let queued = rig.of_kind("task.queued").into_iter().find(|e| e.task.as_deref() == Some(ask.as_str())).unwrap();
    assert_eq!(queued.by, Actor::Doer("d".into()), "the doer asked it");
    assert_eq!(queued.thread.as_deref(), Some("chat-9"));
    let read = rig.folds.read("tasks", &[Key::Name(ask.clone()), Key::Name("parent".into())], 0);
    assert_eq!(read, j4n_core::value::Value::text(&id));
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
}

// ---- the fold ----

#[test]
fn the_fold_of_tasks_rebuilt_from_the_log_is_the_one_the_loop_kept() {
    let q = choice("db", "which?", &[("a", true), ("b", false)]);
    let rig = Rig::tuned(
        vec![
            Scripted::new("ok", vec![vec![Beat::Session("s".into()), Beat::Tool("t".into(), "x".into()), Beat::Spend(0.002), end("fine")]]),
            Scripted::new("asks", vec![vec![Beat::Ask(q), Beat::Hold, end("answered")]]),
            Scripted::new("forever", vec![vec![Beat::Forever]]),
            Scripted::broken("broken", "no"),
        ],
        |c| c.ask_wait_ms = 100,
    );
    let (a, b, c, d) = (rig.spawn("a", "ok"), rig.spawn("b", "asks"), rig.spawn("c", "forever"), rig.spawn("d", "broken"));
    let ask = waiting_on(&rig, &b);
    rig.tasks.ctl(&ask, Ctl::Answer(pick("b")), Actor::Operator).unwrap();
    rig.tasks.ctl(&c, Ctl::Abort, Actor::Operator).unwrap();
    for id in [&a, &b, &c, &d] {
        rig.finished(id);
    }
    rig.tasks.resume(&a, Some("again"), Actor::Operator).unwrap();
    rig.until("t6", "finished", |r| r.state.is_terminal());
    rig.log.sync();
    let mut replayed = Book::new();
    for e in rig.events() {
        if replayed.wants(&e.kind) {
            replayed.apply(&e);
        }
    }
    assert_eq!(replayed.save(), rig.tasks.book().save(), "snapshot or replay: one truth");
    assert_eq!(replayed.rows().len(), 6);
    assert_eq!(rig.folds.state("tasks"), Some(rig.tasks.book().save()));
    assert!(rig.folds.problems().is_empty(), "{:?}", rig.folds.problems());
}

#[test]
fn tasks_that_were_going_when_the_process_ended_are_stopped_as_interrupted_when_it_starts_again() {
    use crate::world::{Scratch, open_log};
    use j4n_core::fold::Folds;
    use j4n_core::log::{Log, New};
    use j4n_core::task::Config;
    use j4n_core::timer::Timers;
    use std::sync::Arc;
    let project = Scratch::new("restart");
    let home = Scratch::new("restart-home");
    let (log, clock): (Log, _) = open_log(&project);
    let spec = serde_json::to_value(TaskSpec::new("go", "d")).unwrap();
    log.append(New::of("task.queued", Actor::Operator, json!({ "spec": spec })).task("t1")).unwrap();
    log.append(New::of("task.started", Actor::Core, json!({ "doer": "d" })).task("t1")).unwrap();
    log.append(New::of("task.queued", Actor::Operator, json!({ "spec": spec })).task("t2")).unwrap();
    log.append(New::of("task.settled", Actor::Core, json!({ "ok": true, "answer": "fine" })).task("t2")).unwrap();
    let folds = Folds::new(&project.join(".j4n/folds"));
    folds.attach(&log).unwrap();
    let timers = Arc::new(Timers::start(&log));
    let cfg = Config::new(&log, &folds, &timers, project.path(), home.path());
    let (tasks, problems) = Tasks::start(cfg).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].to_string().contains("t1") && problems[0].msg.contains("interrupted"), "{problems:?}");
    let t1 = tasks.book().get("t1").unwrap();
    assert_eq!(t1.state, State::Stopped { why: Why::Interrupted });
    assert_eq!(tasks.book().get("t2").unwrap().state, State::Done { ok: true }, "what had finished stays finished");
    assert_eq!(tasks.menu("t1").iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["resume", "abort"]);
    // the numbers go on from where they were
    let doer: Arc<dyn j4n_core::doer::Doer> = Scripted::new("d", vec![vec![end("x")]]);
    drop(tasks);
    drop(clock);
    let (tasks, problems) = {
        let mut cfg = Config::new(&log, &folds, &timers, project.path(), home.path());
        cfg.doers = vec![doer];
        Tasks::start(cfg).unwrap()
    };
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(tasks.spawn(TaskSpec::new("again", "d"), Actor::Operator).unwrap(), "t3");
}


#[test]
fn dropping_the_loop_stops_its_tasks_as_interrupted_and_their_runs_with_them() {
    let doer = Scripted::new("d", vec![vec![Beat::Session("s".into()), Beat::Forever], vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")]]);
    let rig = Rig::new(vec![doer.clone()]);
    let a = rig.spawn("a", "d");
    rig.until_event("the session", |e| e.kind == "run.session");
    let b = rig.spawn("b", "d");
    let ask = waiting_on(&rig, &b);
    let Rig { tasks, log, folds, timers, clock, project, home } = rig;
    let book = tasks.book().clone();
    drop(tasks);
    assert_eq!(book.get(&a).unwrap().state, State::Stopped { why: Why::Interrupted });
    assert_eq!(book.get(&b).unwrap().state, State::Stopped { why: Why::Interrupted });
    assert_eq!(book.get(&ask).unwrap().state, State::Stopped { why: Why::Orphaned });
    assert_eq!(doer.ctls(), vec![RunCtl::Abort, RunCtl::Abort], "their sessions are kept");
    assert_eq!(timers.pending(), 0);
    drop((log, folds, clock, project, home, timers));
}

#[test]
fn many_tasks_at_once_all_end_in_exactly_one_state() {
    let doer = Scripted::new("d", vec![vec![Beat::Session("s".into()), Beat::Tool("t".into(), "x".into()), Beat::Spend(0.001), say("a"), end("a")]]);
    let rig = Rig::new(vec![doer]);
    let ids: Vec<String> = (0..60).map(|_| rig.spawn("go", "d")).collect();
    for id in &ids {
        assert_eq!(rig.finished(id).state, State::Done { ok: true });
    }
    for id in &ids {
        let ends = rig.kinds(id).iter().filter(|k| *k == "task.settled" || *k == "task.stopped").count();
        assert_eq!(ends, 1, "{id}");
    }
    let nums: Vec<u64> = ids.iter().map(|i| i[1..].parse().unwrap()).collect();
    assert_eq!(nums, (1..=60).collect::<Vec<_>>());
}

#[test]
fn a_doer_that_panics_when_asked_or_told_is_refused_and_the_loop_goes_on() {
    use j4n_core::doer::{Doer, Probe, Run, StartCx};
    use j4n_core::table::Problem;
    use std::sync::Arc;
    struct Nameless;
    impl Doer for Nameless {
        fn id(&self) -> &str {
            panic!("no name")
        }
        fn probe(&self) -> Probe {
            Probe::default()
        }
        fn caps(&self) -> Caps {
            Caps::default()
        }
        fn start(&self, _cx: StartCx) -> Result<Box<dyn Run>, Problem> {
            Err(Problem::refused("nameless", "never"))
        }
    }
    let hostile = Scripted::hostile("rude", vec![vec![Beat::Session("s".into()), Beat::Forever]]);
    let rig = Rig::tuned(vec![hostile.clone(), Scripted::new("fine", vec![vec![end("ok")]])], |c| c.doers.push(Arc::new(Nameless)));
    let id = rig.spawn("go", "rude");
    rig.until_event("the session", |e| e.kind == "run.session");
    // asked what it can do, it said nothing it can: so it cannot be steered, and cannot carry on in a session
    let err = rig.tasks.ctl(&id, Ctl::Steer("hurry".into()), Actor::Operator).unwrap_err();
    assert!(err.msg.contains("cannot carry on in a session"), "{err}");
    // told to stop, it panics: the task stops all the same
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Stopped { why: Why::Aborted });
    let other = rig.spawn("go", "fine");
    assert_eq!(rig.finished(&other).state, State::Done { ok: true }, "the loop is still there");
    let lost = rig.spawn("go", "Nameless");
    assert!(failed(&rig, &lost).contains("no doer called"));
    // and the loop said so when it started
    let (project, home) = (crate::world::Scratch::new("nameless"), crate::world::Scratch::new("nameless-home"));
    let (log, _clock) = crate::world::open_log(&project);
    let folds = j4n_core::fold::Folds::new(&project.join(".j4n/folds"));
    let timers = Arc::new(j4n_core::timer::Timers::start(&log));
    let mut cfg = j4n_core::task::Config::new(&log, &folds, &timers, project.path(), home.path());
    cfg.doers.push(Arc::new(Nameless));
    let (_tasks, problems) = Tasks::start(cfg).unwrap();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].msg.contains("asked its name"), "{problems:?}");
}

#[test]
fn words_that_were_still_streaming_when_a_run_was_stopped_or_started_again_are_kept_once() {
    let turns = vec![vec![Beat::Delta("thinking aloud".into()), Beat::Session("s".into()), Beat::Forever], vec![Beat::Forever]];
    let rig = Rig::new(vec![Scripted::with_caps("deaf", Caps { live: false, resume: true, ..Caps::default() }, turns.clone()), Scripted::new("live", turns)]);
    let words = |id: &str| -> Vec<(String, String)> {
        let kind = |e: &j4n_core::log::Ev| if e.data["narration"] == json!(true) { "narration" } else if e.data["partial"] == json!(true) { "partial" } else { "said" };
        rig.of_kind("run.message").iter().filter(|e| e.task.as_deref() == Some(id)).map(|e| (e.data["text"].as_str().unwrap().to_string(), kind(e).to_string())).collect()
    };
    let a = rig.spawn("go", "deaf");
    rig.until_event("the session", |e| e.kind == "run.session");
    rig.tasks.ctl(&a, Ctl::Steer("faster".into()), Actor::Operator).unwrap();
    assert_eq!(words(&a), [("thinking aloud".to_string(), "narration".to_string())], "kept when the run was started again");
    rig.tasks.ctl(&a, Ctl::Abort, Actor::Operator).unwrap();
    assert_eq!(rig.finished(&a).answer, None, "the second run said nothing");
    let b = rig.spawn("go", "live");
    rig.until_event("b's words", |e| e.kind == "run.session" && e.task.as_deref() == Some(b.as_str()));
    rig.tasks.ctl(&b, Ctl::Abort, Actor::Operator).unwrap();
    let row = rig.finished(&b);
    assert_eq!(row.answer.as_deref(), Some("thinking aloud"), "what was streaming is its report");
    assert_eq!(words(&b), [("thinking aloud".to_string(), "partial".to_string())]);
}

#[test]
fn spend_that_is_not_an_amount_is_not_counted() {
    let doer = Scripted::new("d", vec![vec![Beat::Spend(f64::NAN), Beat::Spend(-3.0), Beat::Spend(f64::INFINITY), Beat::Spend(0.002), end("ok")]]);
    let rig = Rig::new(vec![doer]);
    let id = spawn_with(&rig, "d", |s| s.limits.usd_soft = Some(0.01));
    let row = rig.finished(&id);
    assert_eq!((row.state, row.spent, row.capped), (State::Done { ok: true }, 2_000, false));
    let metrics = rig.of_kind("run.metrics");
    assert!(metrics.iter().all(|e| e.data["usd"].as_f64().is_some_and(|u| u >= 0.0 && u.is_finite())), "{metrics:?}");
}

#[test]
fn a_long_list_of_changed_paths_is_kept_by_its_first_part_and_a_count() {
    let paths: Vec<String> = (0..250).map(|n| format!("f{n}.txt")).collect();
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Changed(paths), nothing()]])]);
    let id = rig.spawn("go", "d");
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
    let ev = &rig.of_kind("run.changed")[0];
    assert_eq!((ev.data["paths"].as_array().unwrap().len(), ev.data["more"].clone()), (200, json!(50)));
}

#[test]
fn words_streamed_without_end_are_kept_by_their_last_part() {
    let chunk = "x".repeat(1 << 16);
    let mut beats: Vec<Beat> = (0..24).map(|_| Beat::Delta(chunk.clone())).collect();
    beats.push(Beat::Delta("THE END".into()));
    beats.push(nothing());
    let rig = Rig::new(vec![Scripted::new("d", vec![beats])]);
    let id = rig.spawn("go", "d");
    let answer = rig.finished(&id).answer.unwrap();
    assert!(answer.ends_with("THE END") && answer.len() < (1 << 20) + (1 << 16), "{} bytes", answer.len());
    assert!(answer.len() > 1 << 19, "the last part is kept whole enough to be a report: {} bytes", answer.len());
}

#[test]
fn a_timer_event_that_is_not_the_one_a_task_waits_for_changes_nothing() {
    use j4n_core::log::New;
    let doer = Scripted::new("d", vec![vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")]]);
    let rig = Rig::new(vec![doer]);
    let id = spawn_with(&rig, "d", |s| s.limits.wall_ms = Some(60_000));
    let ask = waiting_on(&rig, &id);
    for (task, tag) in [(&id, "task.wall"), (&ask, "ask.deadline"), (&id, "ask.deadline"), (&ask, "task.wall")] {
        let fired = New::of("timer.fired", Actor::Core, json!({ "id": 987_654, "tag": tag, "due_ms": 0, "data": null })).task(task);
        rig.log.append(fired).unwrap();
    }
    let probe = rig.spawn("go", "d");
    waiting_on(&rig, &probe);
    assert!(matches!(rig.tasks.book().get(&id).unwrap().state, State::Waiting { .. }), "a forged timer stopped nothing");
    assert_eq!(rig.tasks.book().get(&ask).unwrap().state, State::Running);
    rig.tasks.ctl(&ask, Ctl::Answer(Answer::Yes), Actor::Operator).unwrap();
    assert_eq!(rig.finished(&id).state, State::Done { ok: true });
}

#[test]
fn nothing_a_doer_says_after_its_run_is_over_is_heard() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![say("one"), end("one"), say("too late"), Beat::Notice("late".into())], vec![Beat::Fail("broke".into()), end("but done"), say("later")]])]);
    let a = rig.spawn("go", "d");
    assert_eq!(rig.finished(&a).answer.as_deref(), Some("one"));
    let b = rig.spawn("go", "d");
    assert_eq!(failed(&rig, &b), "broke", "an error ends the run, whatever it says next");
    // each event of the loop's own is in the Log, the doer's are not
    std::thread::sleep(std::time::Duration::from_millis(50));
    let said: Vec<_> = rig.of_kind("run.message").iter().map(|e| e.data["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(said, ["one"]);
    assert!(rig.of_kind("run.notice").is_empty());
}

#[test]
fn a_task_that_starts_again_keeps_the_time_it_first_started() {
    let doer = Scripted::new("d", vec![vec![Beat::Session("s".into()), Beat::Forever], vec![end("again")]]);
    let rig = Rig::new(vec![doer]);
    let id = rig.spawn("go", "d");
    rig.until_event("the session", |e| e.kind == "run.session");
    rig.clock.advance(500);
    rig.tasks.ctl(&id, Ctl::Retune(Tune::default()), Actor::Operator).unwrap();
    let row = rig.finished(&id);
    assert_eq!((row.queued_ms, row.started_ms, row.ended_ms), (1_000, Some(1_000), Some(1_500)));
}

#[test]
fn a_question_is_a_read_task_with_a_wall_of_the_time_it_waits() {
    let rig = Rig::tuned(vec![Scripted::new("d", vec![vec![Beat::Ask(approval("q", "make")), Beat::Hold, end("x")]])], |c| c.ask_wait_ms = 90_000);
    let id = rig.spawn("go", "d");
    let ask = waiting_on(&rig, &id);
    let q = rig.tasks.book().get(&ask).unwrap();
    assert_eq!((q.spec.tune.mode, q.spec.limits.wall_ms, q.spec.goal.as_str()), (Mode::Read, Some(90_000), "may I run `make`?"));
    assert_eq!(q.started_ms, Some(1_000));
}

#[test]
fn spending_exactly_the_soft_cap_is_not_past_it() {
    let rig = Rig::new(vec![Scripted::new("d", vec![vec![Beat::Spend(0.01), end("ok")]])]);
    let id = spawn_with(&rig, "d", |s| s.limits.usd_soft = Some(0.01));
    let row = rig.finished(&id);
    assert_eq!((row.state, row.spent, row.capped), (State::Done { ok: true }, 10_000, false));
    assert!(rig.of_kind("task.cap_reached").is_empty());
}

#[test]
fn a_doer_cannot_speak_after_an_error_either() {
    use j4n_core::doer::{Role, RunEv};
    let doer = Scripted::leaking("d", vec![vec![Beat::Fail("broke".into())]]);
    let rig = Rig::new(vec![doer.clone()]);
    let id = rig.spawn("go", "d");
    assert_eq!(failed(&rig, &id), "broke");
    let out = doer.emit();
    assert!(!out.send(RunEv::Message { role: Role::Assistant, text: "still here".into() }));
    assert!(!out.send(RunEv::End { answer: None, todo_left: 0 }));
}

/// The call being written, its size and the tokens so far are flashed to whoever listens and never written.
#[test]
fn an_open_call_and_the_tokens_are_flashed_and_never_written() {
    let rig = Rig::new(vec![Scripted::new(
        "script",
        vec![vec![Beat::Open("0", "write", "", 0), Beat::Open("0", "write", "chess.py", 4200), Beat::Tokens(900), Beat::Tool("write".into(), "chess.py".into()), Beat::ToolEnd("write".into(), "ok".into()), end("done")]],
    )]);
    let sub = rig.log.subscribe("run.*", 100);
    let id = rig.spawn("chess", "script");
    rig.finished(&id);
    let mut flashes = Vec::new();
    while let Some(e) = sub.try_recv() {
        if e.seq == 0 {
            flashes.push((e.kind.clone(), e.data.clone()));
        }
    }
    assert_eq!(
        flashes,
        [
            ("run.open".to_string(), json!({ "id": "0", "name": "write", "target": "", "bytes": 0 })),
            ("run.open".to_string(), json!({ "id": "0", "name": "write", "target": "chess.py", "bytes": 4200 })),
            ("run.tokens".to_string(), json!({ "out": 900 })),
        ]
    );
    assert!(rig.of_kind("run.open").is_empty() && rig.of_kind("run.tokens").is_empty(), "never written");
    assert_eq!(rig.of_kind("run.tool_call").len(), 1, "the settled call is the one in the Log");
}

/// The words said before a call begins are settled as narration when the call opens, not when it is run.
#[test]
fn the_words_before_an_open_call_are_settled_as_it_opens() {
    // the doer stays with the call open and never runs it, so nothing but the opening can have settled the words
    let rig = Rig::new(vec![Scripted::new("script", vec![vec![Beat::Delta("let me write ".into()), Beat::Open("0", "write", "", 0), Beat::Forever]])]);
    let sub = rig.log.subscribe("run.message", 10);
    let id = rig.spawn("chess", "script");
    let said = sub.recv_timeout(std::time::Duration::from_secs(20)).map(|e| e.data["text"].clone());
    assert_eq!(said, Some(json!("let me write ")), "settled while the call was still being written");
    assert!(rig.of_kind("run.tool_call").is_empty(), "the call was never run");
    rig.tasks.ctl(&id, Ctl::Abort, Actor::Operator).unwrap();
    rig.finished(&id);
}
