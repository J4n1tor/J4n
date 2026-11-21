//! A line typed while a task works, with the switch `inbox.mode` off and
//! on, through the binary's own wiring and the scripted `test` doer. The inbox tags, receipts and keeps an item and delivers it at the settled
//! boundary (`Front::plan_step`, which the screen calls when a task settles) as the section `inbox` of the next turn's Brief, starts a `side`
//! item as a task of its own, and, in `auto`, steers a task at once with a line of a rule that has earned it.

use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::Said;
use j4n_core::task::{Mode, State};
use j4n_core::change::Actor;
use j4n_core::log::New;
use serde_json::{Value, json};
use std::fs;
use std::time::{Duration, Instant};

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

/// A world whose operator folder holds `settings` (`id = value` lines); `pack_off` turns the pack out of the harness.
fn rig(tag: &str, settings: &[(&str, &str)], pack_off: bool) -> (Dirs, World) {
    rig_with(tag, settings, pack_off, false)
}

/// [`rig`]; `brief`: the scripted doer takes a Brief with each line (its connector row says `brief = true`), so what a turn was given can be read.
fn rig_with(tag: &str, settings: &[(&str, &str)], pack_off: bool, brief: bool) -> (Dirs, World) {
    let d = Dirs::new(tag);
    if brief {
        fs::create_dir_all(d.j4n_home().join("connectors")).unwrap();
        fs::write(d.j4n_home().join("connectors/test.toml"), "api = 1\n[[connector]]\nid = \"test\"\nis = \"agent\"\ndialect = \"test\"\nroles = [\"scripted\"]\nbrief = true\n").unwrap();
    }
    let mut rows = String::from("api = 1\n");
    for (id, value) in settings {
        rows.push_str(&format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n"));
    }
    fs::write(d.j4n_home().join("settings/inbox.toml"), rows).unwrap();
    if pack_off {
        d.pack_off("inbox");
    }
    let w = open(&d);
    assert!(w.notices.is_empty(), "{:?}", w.notices);
    (d, w)
}

fn dump(w: &World) -> String {
    w.front.log().sync();
    w.front.log().read(0).map(|e| format!("{:>4} {:<16} {} {}", e.seq, e.kind, e.task.as_deref().unwrap_or("-"), e.data)).collect::<Vec<_>>().join("\n")
}

fn events(w: &World, kind: &str) -> Vec<Value> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

fn until(w: &World, what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while !ready() {
        assert!(Instant::now() < end, "never: {what}\n{}", dump(w));
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Start a task with `script` and wait until it is in the state `want` says.
fn start(w: &World, script: &str, want: fn(&State) -> bool) -> String {
    let Said::Started { task } = w.front.say("main", script) else { panic!("{script:?} did not start a task\n{}", dump(w)) };
    until(w, "the state", || w.front.tasks().book().get(&task).is_some_and(|r| want(&r.state)));
    task
}

fn running(s: &State) -> bool {
    matches!(s, State::Running)
}

fn ended(w: &World, task: &str) -> j4n_core::task::Row {
    w.front.tasks().book().wait(task, |r| r.state.is_terminal(), crate::support::PATIENCE).unwrap_or_else(|| panic!("{task} never ended\n{}", dump(w)))
}

/// Every event the inbox wrote or a label of it, so that "nothing" can be asserted.
fn inbox_events(w: &World) -> usize {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind.starts_with("inbox.") || (e.kind == "label" && e.data["subject_kind"] == "inbox_item")).count()
}

/// The item a line made: the last `inbox.item` event.
fn last_item(w: &World) -> Value {
    events(w, "inbox.item").pop().unwrap_or_else(|| panic!("no item\n{}", dump(w)))
}


fn line_while_a_task_runs(w: &World, how: &str) {
    // a line while a task runs steers it at once, whatever it is about
    let task = start(w, "wait 20000; say done", running);
    let said = w.front.say("main", "also rename the parser module?");
    assert_eq!(said, Said::Steered { task: task.clone() }, "{how}\n{}", dump(w));
    until(w, "the steer", || !events(w, "task.steered").is_empty());
    assert_eq!(events(w, "task.steered")[0]["text"], "also rename the parser module?", "{how}");
    // `stop` is no more than a word for it
    assert_eq!(w.front.say("main", "stop"), Said::Steered { task: task.clone() }, "{how}");
    assert_eq!(inbox_events(w), 0, "{how}: the Log has nothing of the inbox\n{}", dump(w));
    w.front.say("main", ":stop");
    ended(w, &task);
    // a line while a task waits for an answer is refused, in the words it always was (an operator at the screen is what keeps the question open)
    let _operator = crate::screen_drive::Operator::sit(w);
    let task = start(w, "ask which file?", |s| matches!(s, State::Waiting { .. }));
    match w.front.say("main", "what time is it?") {
        Said::Refused(why) => assert!(why.ends_with("asks \"which file?\" · c type the answer"), "{how}: {why}"),
        other => panic!("{how}: {other:?}\n{}", dump(w)),
    }
    assert_eq!(inbox_events(w), 0, "{how}");
    w.front.say("main", ":stop");
    ended(w, &task);
}

#[test]
fn with_the_switch_off_a_line_steers_a_running_task_and_a_waiting_one_refuses_it_exactly_as_before() {
    let (_d, w) = rig("inbox-switch-off-line", &[], false);
    line_while_a_task_runs(&w, "pack on, inbox off");
    w.close();
}

#[test]
fn with_the_pack_out_of_the_harness_it_is_the_same() {
    let (_d, w) = rig("inbox-pack-out-harness", &[], true);
    assert!(matches!(w.front.say("main", ":inbox"), Said::Refused(_)), "no command without the pack");
    line_while_a_task_runs(&w, "pack off");
    w.close();
}

#[test]
fn an_unknown_word_of_the_switch_is_off() {
    let (_d, w) = rig("inbox-unknown-word-switch", &[("inbox.mode", "\"yes please\"")], false);
    line_while_a_task_runs(&w, "inbox = a word that is none");
    w.close();
}


#[test]
fn a_line_about_the_work_waits_as_steer_another_subject_is_side_and_an_unsure_one_is_held_and_the_task_is_never_steered() {
    let (_d, w) = rig("inbox-line-about-work", &[("inbox.mode", "\"show\"")], false);
    let task = start(&w, "refactor the config parser module and its error messages; wait 20000; say done", running);
    let cases = [
        ("the config parser errors should name the line", "steer", "overlap-steer"),
        ("how do I rebase a branch onto main?", "side", "overlap-side"),
        ("please look at docs/guide.md for the typos", "side", "overlap-side"),
        ("the parser is fine but check the colour scheme too", "hold", "overlap-hold"),
        ("???", "hold", "no-words"),
    ];
    for (line, tag, rule) in cases {
        let Said::Kept { id, tag: said } = w.front.say("main", line) else { panic!("{line:?} was not kept\n{}", dump(&w)) };
        let item = last_item(&w);
        assert_eq!((said, item["tag"].as_str(), item["rule"].as_str(), item["item"].as_str()), (tag, Some(tag), Some(rule), Some(id.as_str())), "{line:?}: {item}");
        assert_eq!((item["state"].as_str(), item["text"].as_str(), item["task"].as_str(), item["over"].as_str()), (Some("open"), Some(line), Some(task.as_str()), Some("running")), "{item}");
        assert!(item["why"].as_str().is_some_and(|w| !w.is_empty()), "a reason in fixed words: {item}");
    }
    // each has said its receipt in the chat, in the words of the tag
    let notes: Vec<String> = events(&w, "chat.note").iter().filter_map(|n| n["text"].as_str().map(String::from)).collect();
    assert!(notes.iter().any(|n| n.starts_with("i1 → steer — waits for the next boundary of the task")), "{notes:?}");
    assert!(notes.iter().any(|n| n.contains("held: it goes as the next turn")), "{notes:?}");
    assert!(events(&w, "task.steered").is_empty(), "no line steered the task\n{}", dump(&w));
    assert_eq!(w.front.tasks().book().get(&task).unwrap().state, State::Running);
    w.front.say("main", ":stop");
    ended(&w, &task);
    w.close();
}


#[test]
fn the_rules_are_tried_in_their_order_on_whole_words_and_the_words_are_the_packs() {
    let (_d, w) = rig("inbox-order", &[("inbox.mode", "\"show\"")], false);
    let task = start(&w, "refactor the config parser in src/config.rs; wait 20000; say done", running);
    let cases = [
        // a line of only stop words and fillers is a stop; a stop word among others is not
        ("Stop!", "stop", "stop-line"),
        ("abort", "stop", "stop-line"),
        ("no, wait", "stop", "stop-line"),
        ("stop the config parser", "steer", "overlap-steer"),
        // an opener is read before the overlap, as its first word or as the start of the line
        ("note: the config parser is slow", "note", "note-openers"),
        ("Remember to bump the version", "note", "note-openers"),
        ("later: the config parser docs", "note", "note-openers"),
        ("#12 is the config parser bug", "note", "note-openers"),
        ("btw the config parser has a typo", "side", "side-openers"),
        ("also the config parser", "side", "side-openers"),
        ("notebook sizes in the config parser", "steer", "overlap-steer"),
        // overlap: the plural of a word is the word; a path the task did not touch is another subject; a path it did is not
        ("the parsers fail", "steer", "overlap-steer"),
        ("what does src/render.rs do?", "side", "overlap-side"),
        ("check docs/guide.md", "side", "overlap-side"),
        ("what is a monad?", "side", "overlap-side"),
        ("fix the typo in src/config.rs", "steer", "overlap-steer"),
        ("colour scheme", "hold", "overlap-hold"),
    ];
    for (line, tag, rule) in cases {
        let Said::Kept { tag: said, .. } = w.front.say("main", line) else {
            // a stop ends the task: start the next one the same way
            assert_eq!(tag, "stop", "{line:?}\n{}", dump(&w));
            ended(&w, &task_now(&w));
            start(&w, "refactor the config parser in src/config.rs; wait 20000; say done", running);
            continue;
        };
        let item = last_item(&w);
        assert_eq!((said, item["rule"].as_str()), (tag, Some(rule)), "{line:?}: {item}");
    }
    w.front.say("main", ":stop");
    let _ = task;
    w.close();
}

fn task_now(w: &World) -> String {
    w.front.tasks().book().rows().into_iter().rev().find(|r| r.spec.doer == "test").unwrap().id
}


#[test]
fn a_note_is_kept_and_goes_to_no_model_a_pin_is_recorded_and_only_a_note_is_pinned() {
    let (_d, w) = rig("inbox-note-kept-goes", &[("inbox.mode", "\"show\"")], false);
    let task = start(&w, "refactor the config parser; wait 20000; say done", running);
    let Said::Kept { id: note, tag } = w.front.say("main", "note: bump the docs when this is in") else { panic!("{}", dump(&w)) };
    assert_eq!(tag, "note");
    let item = last_item(&w);
    assert_eq!((item["state"].as_str(), item["rule"].as_str(), item["pinned"].as_bool()), (Some("kept"), Some("note-openers"), Some(false)), "{item}");
    let Said::Kept { id: other, .. } = w.front.say("main", "the config parser should keep its comments") else { panic!("{}", dump(&w)) };
    // only a note is pinned
    assert!(matches!(w.front.say("main", &format!(":inbox pin {other}")), Said::Refused(why) if why.contains("only a note is pinned")));
    assert!(matches!(w.front.say("main", ":inbox pin i99"), Said::Refused(why) if why.contains("no item i99")));
    assert_eq!(w.front.say("main", &format!(":inbox pin {note}")), Said::Command("inbox".into()));
    let item = last_item(&w);
    assert_eq!((item["item"].as_str(), item["pinned"].as_bool(), item["state"].as_str()), (Some(note.as_str()), Some(true), Some("kept")), "{item}");
    w.front.say("main", ":stop");
    ended(&w, &task);
    // the next turn: no goal, no Brief and no steer holds the note's words while it is not pinned (a pinned one is delivered)
    w.front.say("main", "say ok");
    until(&w, "the next task", || events(&w, "task.queued").len() == 2);
    for e in events(&w, "task.queued").iter().chain(events(&w, "brief.rendered").iter()).chain(events(&w, "task.steered").iter()) {
        assert!(!e.to_string().contains("bump the docs") && !e.to_string().contains("inbox"), "{e}");
    }
    // a pin is for a Brief: a doer that takes none is not given the note as it is not given any Brief, and the note stays where it is
    assert_eq!(item_now(&w, &note)["state"], "kept");
    // and `:inbox` lists what waits, a pin among it
    w.front.say("main", ":inbox");
    let said = events(&w, "chat.note").pop().unwrap()["text"].as_str().unwrap().to_string();
    assert!(said.starts_with("inbox show —") && said.contains("¶ note kept pinned “note: bump the docs when this is in”"), "{said}");
    w.close();
}


fn stopped_item(w: &World, state: &str) {
    let item = last_item(w);
    assert_eq!((item["tag"].as_str(), item["rule"].as_str(), item["state"].as_str()), (Some("stop"), Some("stop-line"), Some("stopped-task")), "{state}: {item}");
}

#[test]
fn stop_is_sent_at_once_while_the_task_runs_waits_out_a_limit_or_waits_for_an_answer() {
    let (_d, w) = rig("inbox-stop-sent-at", &[("inbox.mode", "\"show\"")], false);
    let _operator = crate::screen_drive::Operator::sit(&w);
    // running
    let task = start(&w, "wait 20000; say done", running);
    assert_eq!(w.front.say("main", "please stop it now"), Said::Command("stop".into()));
    assert!(matches!(ended(&w, &task).state, State::Stopped { .. }));
    stopped_item(&w, "running");
    // cooling: a limit the provider named, waited out by the task
    let task = start(&w, "fail 429 usage limit reached", |s| matches!(s, State::Cooling { .. }));
    assert_eq!(w.front.say("main", "no wait"), Said::Command("stop".into()));
    assert!(matches!(ended(&w, &task).state, State::Stopped { .. }));
    stopped_item(&w, "cooling");
    // waiting for an answer: a stop that the open question does not take as its answer is a stop
    let task = start(&w, "ask which file?", |s| matches!(s, State::Waiting { .. }));
    assert_eq!(w.front.say("main", "cancel that now"), Said::Command("stop".into()), "{}", dump(&w));
    assert!(matches!(ended(&w, &task).state, State::Stopped { .. }));
    stopped_item(&w, "waiting");
    // every one of them has its receipt, and none was steered
    let states: Vec<String> = events(&w, "inbox.item").iter().map(|i| i["state"].as_str().unwrap().to_string()).collect();
    assert_eq!(states, ["stopping", "stopped-task", "stopping", "stopped-task", "stopping", "stopped-task"]);
    assert!(events(&w, "task.steered").is_empty(), "{}", dump(&w));
    w.close();
}

#[test]
fn a_line_that_only_has_a_stop_word_in_it_is_not_a_stop_and_an_approval_still_takes_stop_as_its_no() {
    let (_d, w) = rig("inbox-line-only-has", &[("inbox.mode", "\"show\"")], false);
    let _operator = crate::screen_drive::Operator::sit(&w);
    let task = start(&w, "refactor the config parser; wait 20000; say done", running);
    for line in ["stop using tabs in the config parser", "wait for the config parser tests", "stopping", "cancelled"] {
        assert!(matches!(w.front.say("main", line), Said::Kept { tag, .. } if tag != "stop"), "{line}\n{}", dump(&w));
    }
    assert_eq!(w.front.tasks().book().get(&task).unwrap().state, State::Running);
    w.front.say("main", ":stop");
    ended(&w, &task);
    // the binder goes first: a question of text does not take `stop` as its answer, so it is a stop; any other line it does not take is
    // refused as it was, since the answer is what the task waits for, and the inbox does not file it
    let task = start(&w, "ask which file?", |s| matches!(s, State::Waiting { .. }));
    let before = events(&w, "inbox.item").len();
    assert!(matches!(w.front.say("main", "what time is it?"), Said::Refused(why) if why.ends_with("asks \"which file?\" · c type the answer")));
    assert_eq!(events(&w, "inbox.item").len(), before);
    assert_eq!(w.front.say("main", "stop"), Said::Command("stop".into()), "{}", dump(&w));
    assert!(matches!(ended(&w, &task).state, State::Stopped { .. }));
    // an approval does: `stop` is the refusal it has always been, and `:stop` is the word that stops
    let task = start(&w, "write a.txt hello", |s| matches!(s, State::Waiting { .. }));
    assert!(matches!(w.front.say("main", "stop"), Said::Answered { .. }), "{}", dump(&w));
    ended(&w, &task);
    w.close();
}


/// Labels of the rule `overlap-steer` as the operator's keys would have written them: `kept` of them right, `wrong` of them not.
fn labelled(w: &World, kept: usize, wrong: usize) {
    for (n, label) in std::iter::repeat_n("kept", kept).chain(std::iter::repeat_n("wrong", wrong)).enumerate() {
        let data = json!({ "subject_kind": "inbox_item", "subject": format!("x{n}"), "producer": "overlap-steer", "model": "", "point": "", "label": label, "weight": 1.0 });
        w.front.log().append(New::of("label", Actor::Operator, data)).unwrap();
    }
}

/// Say a line that the rule `overlap-steer` tags until the folds of labels have caught up with `n` of them, and give the item that did.
fn steered_after(w: &World, n: u64) -> Value {
    let end = Instant::now() + crate::support::PATIENCE;
    loop {
        // the line that finds the rule at the floor is steered at once, and that is the end of the count
        match w.front.say("main", "the config parser errors should name the line") {
            Said::Kept { tag: "steer", .. } => {}
            Said::Steered { .. } => return last_item(w),
            other => panic!("{other:?}"),
        }
        let item = last_item(w);
        if item["n"].as_u64() == Some(n) || Instant::now() > end {
            return item;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn a_retag_is_a_label_of_the_rule_and_the_floor_is_forty_labels_at_ninety_five_in_a_hundred() {
    // (kept, wrong, steerable): 39 of 39 is too few; 40 at 0.925 is too low; 38 and 2 is exactly 0.95; 40 of 40
    for (tag, kept, wrong, steerable) in [("a", 39, 0, false), ("b", 37, 3, false), ("c", 38, 2, true), ("d", 40, 0, true), ("e", 0, 0, false)] {
        let (_d, w) = rig(&format!("inbox155{tag}"), &[("inbox.mode", "\"auto\"")], false);
        let task = start(&w, "refactor the config parser and its error messages; wait 20000; say done", running);
        labelled(&w, kept, wrong);
        let item = steered_after(&w, (kept + wrong) as u64);
        assert_eq!((item["steerable"].as_bool(), item["n"].as_u64(), item["tag"].as_str()), (Some(steerable), Some((kept + wrong) as u64), Some("steer")), "{kept}/{wrong}: {item}");
        if kept + wrong > 0 {
            assert!((item["prec"].as_f64().unwrap() - kept as f64 / (kept + wrong) as f64).abs() < 1e-9, "{item}");
        }
        // below the floor nothing steers mid-turn; at it, `auto` steers that one line and a tag other than `steer` is never steerable
        assert_eq!(events(&w, "task.steered").len(), usize::from(steerable), "{}", dump(&w));
        assert!(matches!(w.front.say("main", "how do I rebase a branch onto main?"), Said::Kept { tag: "side", .. }));
        assert_eq!(last_item(&w)["steerable"], false);
        assert_eq!(w.front.tasks().book().get(&task).unwrap().state, State::Running);
        w.front.say("main", ":stop");
        ended(&w, &task);
        w.close();
    }
}

#[test]
fn the_operator_saying_a_tag_is_a_label_wrong_if_it_changed_kept_if_not_and_only_the_first_word_counts() {
    let (_d, w) = rig("inbox-operator-saying-tag", &[("inbox.mode", "\"show\"")], false);
    let task = start(&w, "refactor the config parser and its error messages; wait 20000; say done", running);
    let Said::Kept { id: one, .. } = w.front.say("main", "the config parser errors should name the line") else { panic!() };
    let Said::Kept { id: two, .. } = w.front.say("main", "the config parser errors should name the file") else { panic!() };
    let labels = |w: &World| -> Vec<(String, String, String)> { events(w, "label").iter().filter(|l| l["subject_kind"] == "inbox_item").map(|l| (l["subject"].as_str().unwrap().into(), l["producer"].as_str().unwrap().into(), l["label"].as_str().unwrap().into())).collect() };
    // not a tag, not an item, not a stop
    for bad in [format!(":inbox tag {one} nonsense"), format!(":inbox tag {one} stop"), ":inbox tag i99 side".to_string(), format!(":inbox tag {one}")] {
        assert!(matches!(w.front.say("main", &bad), Said::Refused(_)), "{bad}");
    }
    assert!(labels(&w).is_empty(), "a refused word is no label");
    // saying the tag it has: the rule was right
    w.front.say("main", &format!(":inbox tag {one} steer"));
    // changing it: the rule was wrong, and the item is the operator's now
    w.front.say("main", &format!(":inbox tag {two} side"));
    // a second word on the same item is no second label of the rule
    w.front.say("main", &format!(":inbox tag {two} hold"));
    w.front.say("main", &format!(":inbox tag {one} note"));
    assert_eq!(labels(&w), [(one.clone(), "overlap-steer".into(), "kept".into()), (two.clone(), "overlap-steer".into(), "wrong".into())]);
    let items = events(&w, "inbox.item");
    let now = |id: &str| items.iter().rev().find(|i| i["item"] == id).unwrap().clone();
    let (a, b) = (now(&one), now(&two));
    assert_eq!((a["tag"].as_str(), a["state"].as_str(), a["rule"].as_str(), a["told"].as_bool()), (Some("note"), Some("kept"), Some("overlap-steer"), Some(true)), "{a}");
    assert_eq!((b["tag"].as_str(), b["state"].as_str(), b["why"].as_str(), b["steerable"].as_bool()), (Some("hold"), Some("open"), Some("you said so"), Some(false)), "{b}");
    w.front.say("main", ":stop");
    ended(&w, &task);
    // an item that is over is not tagged again
    w.front.say("main", "wait 20000");
    until(&w, "running", || w.front.tasks().book().rows().iter().any(|r| r.state == State::Running));
    assert_eq!(w.front.say("main", "stop"), Said::Command("stop".into()));
    let stopped = last_item(&w)["item"].as_str().unwrap().to_string();
    assert!(matches!(w.front.say("main", &format!(":inbox tag {stopped} side")), Said::Refused(why) if why.contains("is over")));
    w.close();
}


const WORK: &str = "refactor the config parser module and its error messages; wait 2500; say first";

/// What the settled boundary starts: the screen calls `plan_step` when a task settles, and so does this.
fn boundary(w: &World, task: &str) {
    ended(w, task);
    w.front.plan_step("main");
}

fn tasks(w: &World) -> Vec<j4n_core::task::Row> {
    w.front.tasks().book().rows().into_iter().filter(|r| r.spec.doer == "test").collect()
}

fn brief_of(row: &j4n_core::task::Row) -> String {
    row.spec.input["brief"].as_str().unwrap_or_default().to_string()
}

/// The last event of each item, by its id.
fn item_now(w: &World, id: &str) -> Value {
    events(w, "inbox.item").into_iter().rev().find(|i| i["item"] == id).unwrap_or_else(|| panic!("no item {id}\n{}", dump(w)))
}

#[test]
fn three_steer_lines_are_one_section_of_the_next_brief_numbered_verbatim_and_delivered_once_in_show_and_auto() {
    // odd spaces, quotes, a path, unicode, a trailing mark: none of it may be touched
    let lines = ["The config parser Errors  should name the line", "config parser module: keep  “error messages” naïve  as is?", "config parser error messages: \"don't\"  trim; ok"];
    for mode in ["show", "auto"] {
        let (_d, w) = rig_with(&format!("inbox156{mode}"), &[("inbox.mode", &format!("\"{mode}\""))], false, true);
        let first = start(&w, WORK, running);
        for line in lines {
            assert!(matches!(w.front.say("main", line), Said::Kept { tag: "steer", .. }), "{mode}: {line:?}\n{}", dump(&w));
        }
        // nothing reached the task, and nothing starts while it works
        assert!(events(&w, "task.steered").is_empty() && tasks(&w).len() == 1, "{mode}\n{}", dump(&w));
        boundary(&w, &first);
        let all = tasks(&w);
        assert_eq!(all.len(), 2, "{mode}: one turn carries the three lines\n{}", dump(&w));
        let (next, brief) = (&all[1], brief_of(&all[1]));
        // the same session, the line J4n says (never the operator's words as the goal), and the section after the contract
        assert_eq!(next.spec.follows.as_deref(), Some(first.as_str()), "{mode}");
        assert!(next.spec.goal.contains("section `inbox` of your brief") && !lines.iter().any(|l| next.spec.goal.contains(l)), "{mode}: {}", next.spec.goal);
        let want = format!("## inbox\n1. {}\n2. {}\n3. {}", lines[0], lines[1], lines[2]);
        assert!(brief.contains(&want), "{mode}: {brief}");
        assert!(brief.starts_with("## contract") && brief.find("## contract") < brief.find("## inbox"), "{mode}: {brief}");
        assert_eq!(brief.matches("## inbox").count(), 1, "one section for three lines");
        assert!(events(&w, "brief.rendered").iter().any(|b| b["sections"].as_array().is_some_and(|s| s.iter().any(|n| n == "inbox"))), "{mode}\n{}", dump(&w));
        // the receipts: each item is delivered, to that turn
        for (n, id) in ["i1", "i2", "i3"].iter().enumerate() {
            let item = item_now(&w, id);
            assert_eq!((item["state"].as_str(), item["turn"].as_str(), item["text"].as_str()), (Some("delivered"), Some(next.id.as_str()), Some(lines[n])), "{mode}: {item}");
        }
        // delivered once: the next boundary has nothing to say
        boundary(&w, &next.id);
        assert_eq!(tasks(&w).len(), 2, "{mode}: nothing is delivered twice\n{}", dump(&w));
        w.close();
    }
}

#[test]
fn a_held_line_goes_with_the_steer_lines_a_pinned_note_goes_once_and_one_that_is_not_pinned_never() {
    let (_d, w) = rig_with("inbox-pin", &[("inbox.mode", "\"show\"")], false, true);
    let first = start(&w, WORK, running);
    for (line, tag) in [("note: the  docs want a pin", "note"), ("the config parser errors should name the line", "steer"), ("colour scheme", "hold"), ("note: this one stays here", "note")] {
        assert!(matches!(w.front.say("main", line), Said::Kept { tag: said, .. } if said == tag), "{line}\n{}", dump(&w));
    }
    assert_eq!(w.front.say("main", ":inbox pin i1"), Said::Command("inbox".into()));
    boundary(&w, &first);
    let all = tasks(&w);
    assert_eq!(all.len(), 2, "{}", dump(&w));
    let brief = brief_of(&all[1]);
    assert!(brief.contains("## inbox\n1. the config parser errors should name the line\n2. colour scheme\nNotes the operator pinned for you:\n3. note: the  docs want a pin"), "{brief}");
    assert!(!brief.contains("this one stays here"), "{brief}");
    assert_eq!(item_now(&w, "i1")["state"], "delivered");
    assert_eq!(item_now(&w, "i4")["state"], "kept");
    w.close();
}

#[test]
fn a_pin_alone_starts_nothing_and_rides_in_the_next_turn_the_operator_starts_once() {
    let (_d, w) = rig_with("inbox-alone", &[("inbox.mode", "\"show\"")], false, true);
    let first = start(&w, WORK, running);
    assert!(matches!(w.front.say("main", "note: bump the docs"), Said::Kept { tag: "note", .. }));
    w.front.say("main", ":inbox pin i1");
    boundary(&w, &first);
    assert_eq!(tasks(&w).len(), 1, "a pinned note is no reason for a turn\n{}", dump(&w));
    let Said::Started { task: second } = w.front.say("main", "say ok") else { panic!("{}", dump(&w)) };
    let brief = brief_of(&w.front.tasks().book().get(&second).unwrap());
    assert!(brief.contains("## inbox\nNotes the operator pinned for you:\n1. note: bump the docs"), "{brief}");
    assert_eq!(item_now(&w, "i1")["turn"].as_str(), Some(second.as_str()));
    ended(&w, &second);
    let Said::Started { task: third } = w.front.say("main", "say again") else { panic!("{}", dump(&w)) };
    assert!(!brief_of(&w.front.tasks().book().get(&third).unwrap()).contains("inbox"), "once");
    w.close();
}

#[test]
fn a_stopped_task_starts_no_turn_and_its_lines_ride_with_the_next_one_the_operator_starts() {
    let (_d, w) = rig_with("inbox-stop", &[("inbox.mode", "\"show\"")], false, true);
    let first = start(&w, "refactor the config parser module and its error messages; wait 20000; say first", running);
    assert!(matches!(w.front.say("main", "the config parser errors should name the line"), Said::Kept { tag: "steer", .. }));
    assert!(matches!(w.front.say("main", ":stop"), Said::Command(_)));
    boundary(&w, &first);
    assert_eq!(tasks(&w).len(), 1, "the operator said stop: nothing starts by itself\n{}", dump(&w));
    assert_eq!(item_now(&w, "i1")["state"], "open");
    let Said::Started { task } = w.front.say("main", "say ok") else { panic!("{}", dump(&w)) };
    assert!(brief_of(&w.front.tasks().book().get(&task).unwrap()).contains("## inbox\n1. the config parser errors should name the line"), "{}", dump(&w));
    w.close();
}

#[test]
fn a_doer_without_a_brief_gets_the_numbered_lines_as_the_line_itself() {
    let (_d, w) = rig("inbox-plain", &[("inbox.mode", "\"show\"")], false);
    let first = start(&w, WORK, running);
    w.front.say("main", "the config parser errors should name the line");
    w.front.say("main", "config parser messages:  keep the line");
    boundary(&w, &first);
    let all = tasks(&w);
    assert_eq!(all.len(), 2, "{}", dump(&w));
    assert!(all[1].spec.goal.ends_with("\n\n1. the config parser errors should name the line\n2. config parser messages:  keep the line"), "{}", all[1].spec.goal);
    assert_eq!(all[1].spec.input["brief"], Value::Null);
    w.close();
}

// ---- the mid-turn steer ----

#[test]
fn a_steer_line_of_a_rule_at_the_floor_reaches_the_task_at_once_only_in_auto() {
    // (mode, kept labels, steers now)
    for (tag, mode, kept, steers) in [("a", "auto", 40, true), ("b", "auto", 39, false), ("c", "show", 40, false)] {
        let (_d, w) = rig(&format!("inbox156mid{tag}"), &[("inbox.mode", &format!("\"{mode}\""))], false);
        let task = start(&w, "refactor the config parser and its error messages; wait 20000; say done", running);
        labelled(&w, kept, 0);
        let line = "the config parser errors should name the line";
        let end = Instant::now() + crate::support::PATIENCE;
        let said = loop {
            let said = w.front.say("main", line);
            if matches!(said, Said::Steered { .. }) || last_item(&w)["n"].as_u64() == Some(kept as u64) || Instant::now() > end {
                break said;
            }
            std::thread::sleep(Duration::from_millis(30));
        };
        let item = last_item(&w);
        if steers {
            assert_eq!(said, Said::Steered { task: task.clone() }, "{tag}: {item}");
            until(&w, "the steer", || !events(&w, "task.steered").is_empty());
            assert_eq!(events(&w, "task.steered")[0]["text"], line);
            assert_eq!((item["state"].as_str(), item["turn"].as_str(), item["steerable"].as_bool()), (Some("delivered"), Some(task.as_str()), Some(true)), "{item}");
        } else {
            assert!(matches!(said, Said::Kept { tag: "steer", .. }), "{tag}: {said:?}");
            assert_eq!(item["state"], "open", "{item}");
            assert!(events(&w, "task.steered").is_empty(), "{tag}\n{}", dump(&w));
        }
        w.front.say("main", ":stop");
        ended(&w, &task);
        w.close();
    }
}


#[test]
fn a_side_line_starts_no_task_while_the_first_works_then_one_of_its_own_in_no_session_and_two_run_one_after_the_other() {
    for mode in ["show", "auto"] {
        let (_d, w) = rig(&format!("inbox157{mode}"), &[("inbox.mode", &format!("\"{mode}\""))], false);
        let first = start(&w, "refactor the config parser module; wait 2500; say report A", running);
        let bound = w.front.binding("main").unwrap();
        for line in ["btw say report B", "btw say report C"] {
            assert!(matches!(w.front.say("main", line), Said::Kept { tag: "side", .. }), "{mode}: {line}\n{}", dump(&w));
        }
        assert_eq!(tasks(&w).len(), 1, "{mode}: a side line never touches the running task\n{}", dump(&w));
        assert!(events(&w, "task.steered").is_empty());
        boundary(&w, &first);
        let all = tasks(&w);
        assert_eq!(all.len(), 2, "{mode}: one side line at a boundary\n{}", dump(&w));
        let b = &all[1];
        // its own task: the line as the goal, the thread's binding, no session of the first
        assert_eq!((b.spec.goal.as_str(), b.spec.thread.as_deref(), b.spec.follows.as_deref(), b.spec.doer.as_str()), ("btw say report B", Some("main"), None, bound.doer.as_str()), "{mode}");
        assert_eq!(b.spec.tune.model, bound.model);
        let item = item_now(&w, "i1");
        assert_eq!((item["state"].as_str(), item["turn"].as_str()), (Some("started"), Some(b.id.as_str())), "{item}");
        assert_eq!(item_now(&w, "i2")["state"], "open");
        // two reports, two answers
        assert_eq!(ended(&w, &first).answer.as_deref(), Some("report A"));
        boundary(&w, &b.id);
        assert_eq!(tasks(&w).len(), 3, "{mode}\n{}", dump(&w));
        let c = tasks(&w).pop().unwrap();
        assert_eq!((c.spec.goal.as_str(), c.spec.follows.as_deref()), ("btw say report C", None), "{mode}");
        assert_eq!(ended(&w, &b.id).answer.as_deref(), Some("btw say report B"));
        boundary(&w, &c.id);
        assert_eq!(ended(&w, &c.id).answer.as_deref(), Some("btw say report C"));
        assert_eq!(tasks(&w).len(), 3, "{mode}: nothing is started twice\n{}", dump(&w));
        w.close();
    }
}

#[test]
fn with_the_switch_off_nothing_is_started_by_the_inbox() {
    let (_d, w) = rig("inbox-off", &[], false);
    let first = start(&w, "refactor the config parser module; wait 500; say report A", running);
    w.front.say("main", "btw say report B");
    boundary(&w, &first);
    assert_eq!(tasks(&w).len(), 1, "{}", dump(&w));
    assert_eq!(inbox_events(&w), 0);
    w.close();
}

#[test]
fn what_is_due_is_claimed_by_the_first_caller_until_it_is_sent_or_given_back() {
    let (_d, w) = rig("inbox-claim", &[("inbox.mode", "\"show\"")], false);
    let inbox = w.front.cfg().inbox.clone().unwrap();
    let task = start(&w, "refactor the config parser module; wait 20000; say first", running);
    w.front.say("main", "the config parser errors should name the line");
    w.front.say("main", "btw say report B");
    w.front.say("main", ":stop");
    ended(&w, &task);
    // a stopped task starts nothing, so the items are there for the callers
    let one = inbox.due("main", true).expect("the steer line is due");
    assert_eq!(one.ids, ["i1"]);
    assert!(inbox.due("main", true).is_none() && inbox.due("main", false).is_none(), "claimed");
    inbox.back(&one.ids);
    let again = inbox.due("main", true).expect("given back, it is due again");
    assert_eq!(again.ids, ["i1"]);
    inbox.sent(&again.ids, "delivered", "t9");
    assert!(inbox.due("main", false).is_none(), "delivered is over");
    // the side item the same
    assert_eq!(inbox.side("main"), Some(("i2".into(), "btw say report B".into())));
    assert_eq!(inbox.side("main"), None, "claimed");
    inbox.back(&["i2".into()]);
    assert_eq!(inbox.side("main").map(|s| s.0), Some("i2".into()));
    inbox.sent(&["i2".into()], "started", "t10");
    assert_eq!(inbox.side("main"), None, "started is over");
    // another thread's items are not this one's
    assert!(inbox.due("elsewhere", false).is_none() && inbox.side("elsewhere").is_none());
    w.close();
}
