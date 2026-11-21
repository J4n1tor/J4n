//! The quick look and the catalogue, with no model: the cheap doer is a script that plays back a recorded answer. The first part reads the answers
//! haiku and sonnet gave to a tuning set of 44 lines (`fixtures/quick-look/p0-recorded.json`), which is never a gate: they show that the
//! code takes each answer as it should and that the set's own counts come out of them. The second is what code decides itself: the reads rule, the
//! standing rule, the lines that never reach the look, and a look that cannot be made.

use crate::front_cases::{Fx, chatty, fx};
use crate::scripted::{Beat, Scripted, end, say};
use crate::world::Scratch;
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::catalogue::{Roster, build};
use j4n_core::front::look::{Decision, Looked, changes_files, named_bytes, parse, scouted, standing_rule, taken};
use j4n_core::pack::{Info, Origin};
use serde_json::{Value, json};
use std::time::Duration;

fn looking(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.cheap.doer = Some("cheap".into());
}

fn fx_with(look: &str) -> Fx {
    fx(vec![chatty("claude", &["done", "done"]), chatty("cheap", &[look])], looking)
}

fn kinds(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

fn work(f: &Fx) -> usize {
    f.tasks().iter().filter(|r| r.spec.role.as_deref() == Some("work")).count()
}

fn recorded() -> Vec<Value> {
    let all: Value = serde_json::from_str(include_str!("../../../../fixtures/quick-look/p0-recorded.json")).unwrap();
    all["cases"].as_array().unwrap().clone()
}

#[test]
fn the_recorded_answers_of_the_pilot_are_taken_as_the_code_says_and_every_line_goes_on_to_main_as_one_turn() {
    let cases = recorded();
    assert_eq!(cases.len(), 44);
    let seen = cases.len() - 1;
    let (mut false_splits, mut missed) = (0, 0);
    for c in &cases {
        let (id, line, label) = (c["id"].as_str().unwrap(), c["line"].as_str().unwrap(), c["label"].as_str().unwrap());
        let answer = c["recorded"]["haiku"].to_string();
        let f = fx_with(&answer);
        f.turn(line);
        let looked = kinds(&f, "orchestrate.looked");
        // a bare yes is read like any plain line (no word of it is looked at): with nothing before it, it is a line of its own
        assert_eq!(looked.len(), 1, "{id}: {}", f.rig.dump());
        let said = looked[0]["decision"].as_str().unwrap();
        assert_eq!(said, c["recorded"]["haiku"]["decision"].as_str().unwrap(), "{id}");
        assert!(looked[0]["error"].is_null() && looked[0]["model"].is_string() && looked[0]["ms"].is_number(), "{id}: {}", looked[0]);
        // the line is Main's own turn whatever was seen: one turn of the role work in the thread, no chat
        assert_eq!(work(&f), 1, "{id}");
        assert!(f.front.chats().is_empty(), "{id}");
        if label != "split" && said == "split" {
            false_splits += 1;
        }
        if label == "split" && said != "split" {
            missed += 1;
        }
    }
    // the set's own row for haiku, first repeat: no false split, five missed (the bare yes is no split line)
    assert_eq!((false_splits, missed, seen), (0, 5, 43));
}

#[test]
fn a_split_whose_chats_only_read_is_one_lane_unless_the_files_named_are_big_and_a_standing_rule_is_never_a_reply() {
    let split = |writes: bool| Looked { decision: Decision::Split, chats: vec![j4n_core::front::look::LookChat { goal: "read it".into(), writes, doer: "claude".into(), model: "haiku".into() }], features: vec![], why: String::new() };
    assert_eq!(taken(&split(false), "read the four reports", false).0, Decision::One);
    assert_eq!(taken(&split(false), "read the four reports", true).0, Decision::Split, "big files are worth a chat");
    assert_eq!(taken(&split(true), "change two modules", false).0, Decision::Split, "a writing chat is a split");
    let reply = Looked { decision: Decision::Answer, chats: vec![], features: vec![], why: String::new() };
    assert_eq!(taken(&reply, "from now on, whenever the context gets above 70 %, compact it", false).0, Decision::One);
    assert_eq!(taken(&reply, "what did the last task cost?", false).0, Decision::Answer);
    for line in ["from now on, always run the tests first", "Whenever you finish, say so", "ok. always keep commits small"] {
        assert!(standing_rule(line), "{line}");
    }
    for line in ["what does the always-on gate do?", "yes, go on", "fix the typo"] {
        assert!(!standing_rule(line), "{line}");
    }
    // the bytes the line names are counted by code, from the files
    let dir = Scratch::new("look-big");
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::write(dir.join("docs/a.md"), vec![b'x'; 150_000]).unwrap();
    std::fs::write(dir.join("docs/b.md"), vec![b'x'; 100_000]).unwrap();
    assert_eq!(named_bytes(dir.path(), "read docs/a.md and summarise"), 150_000);
    assert_eq!(named_bytes(dir.path(), "read docs/a.md and docs/b.md"), 250_000);
    assert_eq!(named_bytes(dir.path(), "read /etc/passwd and ../x.md and nothing/there.md"), 0, "outside the project or not there counts nothing");
}

fn said(decision: Decision) -> Looked {
    Looked { decision, chats: vec![], features: vec![], why: "one piece".into() }
}

fn sized(list: &[(&str, u64)]) -> Vec<(String, u64)> {
    list.iter().map(|(p, n)| (p.to_string(), *n)).collect()
}

#[test]
fn a_read_of_more_than_read_big_that_the_look_called_one_or_an_answer_is_cut_into_read_chats_and_a_read_of_less_or_work_that_writes_is_not() {
    let big = 200_000;
    let line = "read archive/2023/, archive/2024/ and archive/2025/ and write me one paragraph per year";
    let named = sized(&[("archive/2023/", 250_000), ("archive/2024/", 250_000), ("archive/2025/", 140_000)]);
    for d in [Decision::One, Decision::Answer] {
        let (cut, why) = scouted(&said(d), line, &named, big, 4).expect("a big read is split");
        assert_eq!(cut.decision, Decision::Split);
        // 250 + 250 + 140 kB: no two fit in 200 kB, so one chat for each path; none writes
        assert_eq!(cut.chats.len(), 3);
        assert!(cut.chats.iter().all(|c| !c.writes) && cut.chats[0].goal.starts_with("read archive/2023/ (250 kB)"), "{:?}", cut.chats);
        assert_eq!(why, "named 640 kB > read_big: code split the read");
    }
    // paths that are small together share a chat, up to read_big
    let many = sized(&[("a.md", 90_000), ("b.md", 90_000), ("c.md", 90_000), ("d.md", 90_000), ("e.md", 90_000)]);
    let (cut, _) = scouted(&said(Decision::One), "read a.md b.md c.md d.md e.md", &many, big, 4).unwrap();
    assert_eq!(cut.chats.iter().map(|c| c.goal.split(" (").next().unwrap().matches(".md").count()).collect::<Vec<_>>(), [2, 2, 1], "{:?}", cut.chats);
    // more chats than the cap: the paths are shared out by weight, none is dropped
    let (cut, _) = scouted(&said(Decision::One), line, &named, big, 2).unwrap();
    assert_eq!(cut.chats.len(), 2);
    assert!(["archive/2023/", "archive/2024/", "archive/2025/"].iter().all(|p| cut.chats.iter().any(|c| c.goal.contains(p))), "{:?}", cut.chats);
    // not split: small files, a split the look made, work that changes files, a standing rule, and a path that is not there
    assert!(scouted(&said(Decision::One), line, &sized(&[("archive/2023/", 150_000)]), big, 4).is_none());
    assert!(scouted(&said(Decision::Split), line, &named, big, 4).is_none());
    for writing in ["fix the dates in archive/2023/ and archive/2024/", "rename every file in archive/2023/", "read archive/2023/ and write the summary to docs/summary.md", "from now on read archive/2023/ first"] {
        assert!(scouted(&said(Decision::One), writing, &named, big, 4).is_none(), "{writing}");
    }
    assert!(scouted(&said(Decision::One), "read nothing/there.md", &sized(&[("nothing/there.md", 0)]), big, 4).is_none());
    for reading in ["summarize each chapter of docs/handbook/ in two sentences", "go through everything in exports/ and tell me which export has the most refunds", "make me a glossary of docs/handbook/", "write me one paragraph per year"] {
        assert!(!changes_files(reading), "{reading}");
    }
    for writing in ["Fix the typo", "write to docs/a.md", "please update the readme", "add a test"] {
        assert!(changes_files(writing), "{writing}");
    }
}

#[test]
fn a_standing_rule_the_look_called_an_answer_is_worked_by_main_and_the_log_says_it_was_clamped() {
    let f = fx_with(r#"{"decision":"answer","chats":[],"features":[],"why":"it is acknowledged"}"#);
    f.turn("from now on, whenever the context gets above 70 %, compact it");
    let l = &kinds(&f, "orchestrate.looked")[0];
    assert_eq!((l["decision"].as_str(), l["taken"].as_str()), (Some("answer"), Some("one")), "{l}");
    assert!(l["clamped"].as_str().is_some_and(|c| c.contains("standing rule")), "{l}");
    assert!(f.notes().iter().any(|n| n.starts_with("read as one") && n.contains("standing rule")), "{:?}", f.notes());
    assert_eq!(work(&f), 1);
}

#[test]
fn the_look_is_made_only_for_a_plain_line_in_main_under_the_word_and_the_switch() {
    // off by default, and under single
    for tune in [(|c: &mut Cfg| c.orchestrate.look = false) as fn(&mut Cfg), |c| c.lanes = Switch::Single] {
        let f = fx(vec![chatty("claude", &["a"]), chatty("cheap", &["{}"])], |c| {
            looking(c);
            tune(c);
        });
        f.turn("fix the typo in the readme");
        assert_eq!(f.count("orchestrate.looked"), 0);
        assert!(f.tasks().iter().all(|r| r.spec.role.as_deref() != Some("look")));
    }
    // a command and a line typed to a chat never reach it; a bare yes and a line with a doer's name in it are plain lines, and are read (no word of them is matched)
    let f = fx_with(r#"{"decision":"one","chats":[],"features":[],"why":"x"}"#);
    f.turn("fix the typo in the readme");
    assert_eq!(f.count("orchestrate.looked"), 1);
    for line in [":grant", "@c1 and the lexer"] {
        f.say(line);
    }
    assert_eq!(f.count("orchestrate.looked"), 1, "{:?}", f.notes());
    // a line above may have started a task: it is over before the next line, or a loaded machine makes that line a steer
    for r in f.tasks() {
        f.rig.finished(&r.id);
    }
    // the look's task is no session of Main's: its next line goes on in the session of its first turn
    let second = f.turn("and the lexer too");
    let before = f.tasks().into_iter().find(|r| Some(r.id.as_str()) == second.spec.follows.as_deref());
    assert!(before.is_some_and(|r| r.spec.role.as_deref() == Some("work")), "{}", f.rig.dump());
}

#[test]
fn a_look_that_cannot_be_made_or_read_leaves_the_line_to_main_as_typed_and_says_why() {
    // an answer that is no JSON, a decision that is none of the three
    for bad in ["I would work this myself.", r#"{"decision":"maybe","chats":[],"features":[],"why":"x"}"#] {
        let f = fx_with(bad);
        f.turn("fix the typo in the readme");
        let l = &kinds(&f, "orchestrate.looked")[0];
        assert!(l["error"].is_string() && l["decision"].is_null(), "{l}");
        assert_eq!(work(&f), 1, "the line went to Main");
        assert!(f.notes().iter().any(|n| n.starts_with("the line was not read")), "{:?}", f.notes());
    }
    // no cheap tier named
    let f = fx(vec![chatty("claude", &["a"])], |c| {
        looking(c);
        c.cheap.doer = None;
    });
    f.turn("fix the typo in the readme");
    let l = &kinds(&f, "orchestrate.looked")[0];
    assert!(l["error"].as_str().is_some_and(|e| e.contains("no cheap tier")), "{l}");
    assert_eq!(work(&f), 1);
}

#[test]
fn the_answer_is_read_from_the_first_json_object_and_what_is_badly_formed_is_left_out() {
    let l = parse("```json\n{\"decision\":\"split\",\"chats\":[{\"goal\":\"a\",\"writes\":false,\"doer\":\"claude\",\"model\":\"haiku\"},\"x\",{\"goal\":\"b\"}],\"features\":[\"kit\",3],\"why\":\"two parts\"}\n```").unwrap();
    assert_eq!((l.decision, l.chats.len(), l.features.clone()), (Decision::Split, 2, vec!["kit".to_string()]));
    assert!(!l.chats[0].writes && l.chats[1].writes, "a chat that does not say it reads is taken as writing");
    let five = format!("{{\"decision\":\"split\",\"chats\":[{}]}}", [r#"{"goal":"g"}"#; 6].join(","));
    assert_eq!(parse(&five).unwrap().chats.len(), 4, "at most four");
    assert!(parse("no").is_err() && parse("{").is_err());
}

fn info(id: &str, on: bool, summary: &str, use_when: &str, words: &[&str]) -> Info {
    Info { id: id.into(), version: "0.0.0".into(), summary: summary.into(), use_when: use_when.into(), tag: "", origin: Origin::Builtin, on, note: String::new(), adds: String::new(), widens: false, pin: String::new(), words: words.iter().map(|w| w.to_string()).collect(), ready: None, keyed: Vec::new() }
}

#[test]
fn the_catalogue_leads_with_when_to_use_a_pack_marks_the_off_ones_and_is_shortened_to_its_cap() {
    let long = "a summary written for a developer that goes on and on about the kernel and the switch and the way it was built and measured";
    let packs: Vec<Info> = (0..12).map(|n| info(&format!("pack{n}"), n % 2 == 0, long, "a line asks for something this pack does", &[":go", ":stop"])).chain([info("base", true, "core", "", &[]), info("test", true, "offline", "", &[])]).collect();
    let roster = [Roster { doer: "claude".into(), models: vec!["haiku".into(), "opus".into()] }, Roster { doer: "pi".into(), models: vec![] }];
    let whole = build(&packs, &roster, &["debug-cause".into()], 100_000);
    assert!(whole.text.contains("- pack1 [off]: a summary") && whole.text.contains("use when a line asks for something this pack does (:go :stop)"), "{}", whole.text);
    assert!(!whole.text.contains("- base") && !whole.text.contains("- test"), "the core words and the offline tool are no feature a line asks for");
    assert!(whole.text.contains("claude [haiku (cheap), opus (strong, dear)]; pi;") && whole.text.contains("Skills that fit this project: debug-cause."));
    assert_eq!(whole.packs.len(), 12);
    // a cap takes the commands first, then the summaries, then the skills, and never the roster or a pack that is on
    let skills = ["debug-cause".to_string()];
    let at = |cap: usize| build(&packs, &roster, &skills, cap);
    let mut seen = Vec::new();
    for cap in (1..=whole.tokens).rev() {
        let c = at(cap);
        let level = (c.text.contains(":go"), c.text.contains("a summary"), c.text.contains("Skills"));
        if seen.last() != Some(&level) {
            seen.push(level);
        }
        assert!(c.text.contains("use when") && c.text.contains("claude ["), "{}", c.text);
    }
    assert_eq!(seen, [(true, true, true), (false, true, true), (false, false, true), (false, false, false)]);
    let least = build(&packs, &roster, &["debug-cause".into()], 1);
    assert!(least.text.contains("claude [") && !least.text.contains("Skills") && !least.packs.iter().any(|p| packs.iter().any(|i| i.id == *p && !i.on)), "{}", least.text);
}

#[test]
fn the_look_alone_answers_as_json_with_what_code_takes_it_as_and_sends_nothing_to_a_chat() {
    let f = fx(vec![chatty("claude", &["a"]), chatty("haiku-doer", &[r#"{"decision":"split","chats":[{"goal":"read the reports","writes":false,"doer":"claude","model":"haiku"}],"features":["kit","nothing"],"why":"reads"}"#])], |c| {
        c.doer = Some("claude".into());
    });
    let said = f.front.look_once("look-1", "read docs/a.md and summarise it", Some("haiku-doer"), None, &Default::default()).unwrap();
    assert_eq!((said["decision"].as_str(), said["taken"].as_str()), (Some("split"), Some("one")), "{said}");
    assert!(said["clamped"].is_string() && said["chats"][0]["writes"] == false && said["why"] == "reads", "{said}");
    assert!(said["ms"].is_number() && said["usd"].is_number() && said["tokens"].as_u64().is_some_and(|t| t > 0) && said["doer"] == "haiku-doer", "{said}");
    assert_eq!(said["features"], serde_json::json!([]), "a feature the catalogue does not list is dropped");
    // the thread is its own: Main has no turn, no session, no chat and no note of it
    assert!(f.tasks().iter().all(|r| r.spec.thread.as_deref() == Some("look-1") && r.spec.role.as_deref() == Some("look")), "{}", f.rig.dump());
    assert!(f.front.chats().is_empty() && f.front.binding("main").is_none() && f.count("chat.note") == 0);
    // a doer that is not there is an error in words, not a panic
    assert!(f.front.look_once("look-2", "x", Some("nobody"), None, &Default::default()).unwrap_err()["error"].as_str().is_some_and(|e| e.contains("not one J4n has")));
}

#[test]
fn the_catalogue_page_is_made_of_the_data_of_the_text_and_says_what_a_cap_left_out_and_how_to_turn_the_look_on() {
    let long = "a summary written for a developer that goes on and on";
    let packs: Vec<Info> = (0..6).map(|n| info(&format!("pack{n}"), n % 2 == 0, long, "a line asks for something this pack does", &[":go"])).chain([info("base", true, "core", "", &[])]).collect();
    let roster = [Roster { doer: "claude".into(), models: vec!["haiku".into(), "opus".into()] }];
    let whole = build(&packs, &roster, &[], 100_000);
    let rows = j4n_core::front::catalogue::rows(&packs, &roster, &whole, false);
    let ids: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(ids, ["look", "pack:pack0", "pack:pack1", "pack:pack2", "pack:pack3", "pack:pack4", "pack:pack5", "doer:claude"], "the core words are no row, and the doers are");
    // the packs the text names are the rows that are not marked, whatever the cap
    for cap in [100_000, 120, 1] {
        let told = build(&packs, &roster, &[], cap);
        let rows = j4n_core::front::catalogue::rows(&packs, &roster, &told, false);
        let named: Vec<&str> = rows.iter().filter(|r| r.key.starts_with("pack:") && !r.cells[2].text.starts_with("not told")).map(|r| &r.key["pack:".len()..]).collect();
        assert_eq!(named, told.packs.iter().map(String::as_str).collect::<Vec<_>>(), "cap {cap}");
    }
    // off, the first row says how to turn it on and the key does; on, the key turns it off, and `u` takes it back either way
    let off = &rows[0];
    assert!(off.cells[1].text == "off" && off.cells[2].text.contains("⏎ turns it on"), "{off:?}");
    let on = &j4n_core::front::catalogue::rows(&packs, &roster, &whole, true)[0];
    let value = |r: &j4n_proto::Row| match &r.actions[0].intent { j4n_proto::Intent::Set { id, value, .. } => (id.clone(), value.clone()), other => panic!("{other:?}") };
    assert_eq!((value(off), value(on)), ((String::from("orchestrate.look"), json!(true)), (String::from("orchestrate.look"), json!(false))));
    assert!(off.actions[1].key == "u" && on.cells[1].text == "on");
}

/// A look whose cheap doer plays `beats`, waiting `first` for its first reply and `answer` for its answer after it: the error the look wrote, if any.
fn look_error(beats: Vec<Beat>, first: u64, answer: u64) -> Option<String> {
    let f = fx(vec![chatty("claude", &["done"]), Scripted::new("cheap", vec![beats])], |c| {
        looking(c);
        c.look_wait = [Duration::from_millis(first), Duration::from_millis(answer)];
    });
    f.turn("fix the typo in the readme");
    let looked = kinds(&f, "orchestrate.looked");
    assert_eq!(looked.len(), 1, "{}", f.rig.dump());
    looked[0]["error"].as_str().map(String::from)
}

#[test]
fn a_look_waits_for_its_first_reply_apart_from_its_answer() {
    // on a slow service the look had no reply in its time, counted from its start, and the split of the whole line was lost
    let one = json!({ "decision": "one", "chats": [], "features": [], "why": "one piece" }).to_string();
    let slow = vec![Beat::Session("c".into()), Beat::Pause(700), Beat::Delta("{".into()), say(&one), end(&one)];
    assert_eq!(look_error(slow, 5_000, 300), None, "the first reply came after the answer's wait and within the first's");
    let mute = look_error(vec![Beat::Session("c".into()), Beat::Served("claude-haiku-5-5", "anthropic"), Beat::Tokens(0), Beat::Forever], 400, 5_000).unwrap_or_default();
    assert!(mute.contains("no reply came in 400ms"), "{mute}");
    let rambles = look_error(vec![Beat::Session("c".into()), Beat::Delta("{".into()), Beat::Pause(3_000), end(&one)], 5_000, 300).unwrap_or_default();
    assert!(rambles.contains("did not answer in time"), "the answer's wait counts from the first reply: {rambles}");
}
