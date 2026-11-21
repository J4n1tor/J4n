//! Seats: a brief of Main's may name one, the seat fills what the brief leaves empty and narrows its access (never widens it), the chat is given the seat's
//! words, and `chat.seated` says what was done. With `crew.on` off the field is not read and a chat is started as before.

use crate::branch_validate::{A, B, Held, doer};
use crate::front_cases::{Fx, chatty, fx_placed};
use crate::split_cases::{LINE, LOOK, answer, brief, chat_tasks, events, say, settings, settle};
use j4n_core::crew::{Access, Crew, Seat, narrow};
use j4n_core::front::{Cfg, Said};
use j4n_core::front::split::{Brief, Env, take};
use j4n_core::task::Mode;
use serde_json::{Value, json};
use std::sync::Arc;

pub fn seat(id: &str, mode: Option<&str>, paths: &[&str]) -> Seat {
    Seat { id: id.into(), doc: None, role: id.into(), personality: format!("words of {id}"), mode: mode.map(String::from), paths: paths.iter().map(|p| p.to_string()).collect(), tools_allow: vec![], tools_deny: vec![], doer: String::new(), model: String::new(), reports: String::new() }
}

fn access(write: bool, reads: &[&str], writes: &[&str]) -> Access {
    Access { write, reads: reads.iter().map(|p| p.to_string()).collect(), writes: writes.iter().map(|p| p.to_string()).collect() }
}

fn words(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

/// The briefs of an answer with one chat, `fill` changing its JSON, read against `seats`.
fn taken(fill: impl FnOnce(&mut Value), seats: Option<&[Seat]>) -> Result<Brief, String> {
    let mut b = brief("one", false, &A, &[]);
    fill(&mut b);
    take(&answer(vec![b]), &Env { doers: &["pi".to_string(), "plain".to_string()], packs: &[], checks: &[], seats }).map(|(mut v, _)| v.remove(0))
}

// ---- access: a seat narrows and never widens, one case per field ----

#[test]
fn a_seat_that_only_reads_makes_a_read_chat_of_a_write_brief_and_a_write_seat_never_makes_a_read_chat_write() {
    let (a, notes) = narrow(&seat("r", Some("read"), &[]), access(true, &["src/a"], &["src/a/1.rs"])).unwrap();
    assert_eq!(a, access(false, &["src/a"], &[]));
    assert_eq!(notes, ["mode: it only reads"]);
    // the seat that writes leaves a read brief as it is, and a seat with no mode leaves the brief's
    for mode in [Some("write"), None] {
        let (a, notes) = narrow(&seat("w", mode, &[]), access(false, &["src/a"], &[])).unwrap();
        assert_eq!((a, notes), (access(false, &["src/a"], &[]), vec![]));
    }
    // through the brief: a seat that reads and a brief that writes is a chat that reads
    let b = taken(|b| *b = brief("one", true, &A, &["src/a/1.rs"]), Some(&[seat("r", Some("read"), &[])])).unwrap();
    assert_eq!((b.write, b.writes), (true, words(&["src/a/1.rs"])), "no seat named: the brief stands as it was written");
    let b = taken(
        |b| {
            *b = brief("one", true, &A, &["src/a/1.rs"]);
            b["seat"] = json!("r");
        },
        Some(&[seat("r", Some("read"), &[])]),
    )
    .unwrap();
    assert_eq!((b.write, b.writes.is_empty()), (false, true));
}

#[test]
fn a_seats_paths_cut_the_reads_and_the_writes_of_the_brief_by_whole_folders_and_never_add_one() {
    let s = seat("s", None, &["src/a/", "docs"]);
    // inside a seat path stays; a folder that holds a seat path becomes the seat path; a folder that only shares a prefix goes
    let (a, notes) = narrow(&s, access(true, &["src/a/1.rs", "src", "src/ab/1.rs", "README.md"], &["src/a/2.rs", "docs/x.md", "src/b/1.rs"])).unwrap();
    assert_eq!(a.reads, words(&["src/a/1.rs", "src/a"]), "{a:?}");
    assert_eq!(a.writes, words(&["src/a/2.rs", "docs/x.md"]), "{a:?}");
    assert_eq!(notes, ["paths: cut to the paths of the seat", "writes: cut to the paths of the seat"]);
    // a list the brief left empty stays empty: the seat's paths are a limit, not a gift
    let (a, notes) = narrow(&s, access(false, &[], &[])).unwrap();
    assert_eq!((a, notes), (access(false, &[], &[]), vec![]));
    // nothing of the brief inside the seat: the brief is refused
    let why = narrow(&s, access(true, &["src/a/1.rs"], &["src/c/1.rs"])).unwrap_err();
    assert!(why.contains("none of its writes") && why.contains("`s`"), "{why}");
    let why = narrow(&s, access(false, &["src/c/1.rs"], &[])).unwrap_err();
    assert!(why.contains("none of its paths"), "{why}");
    // and a brief that is inside the seat is left whole, with no note
    let (a, notes) = narrow(&s, access(true, &["src/a/1.rs"], &["src/a/2.rs"])).unwrap();
    assert_eq!((a, notes), (access(true, &["src/a/1.rs"], &["src/a/2.rs"]), vec![]));
}

#[test]
fn a_seats_tools_are_written_down_as_they_are_and_a_tool_cannot_be_both_allowed_and_denied() {
    let mut s = seat("t", None, &[]);
    s.tools_allow = words(&["read", "grep"]);
    s.tools_deny = words(&["write"]);
    assert!(s.check(1200).is_ok());
    let seated = s.seated(vec![]);
    assert_eq!((seated.tools_allow, seated.tools_deny), (words(&["read", "grep"]), words(&["write"])));
    s.tools_deny = words(&["grep"]);
    assert!(s.check(1200).unwrap_err().contains("both allowed and denied"));
    s.tools_deny = words(&["Not A Tool"]);
    assert!(s.check(1200).unwrap_err().contains("not the name of a tool"));
}

#[test]
fn the_brief_names_the_doer_and_the_model_and_the_seat_only_fills_what_it_left_empty() {
    let mut s = seat("d", Some("read"), &[]);
    (s.doer, s.model) = ("pi".into(), "small".into());
    // the brief says nothing of mode, doer or model: the seat's stand
    let b = taken(|b| *b = json!({ "id": "one", "goal": "g", "done": "d", "paths": [], "seat": "d" }), Some(&[s.clone()])).unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_str(), b.write), ("pi", "small", false));
    // the brief names a doer and a model of its own: they stand, and the seat's model does not follow a doer that is not its own
    let b = taken(|b| *b = json!({ "id": "one", "goal": "g", "done": "d", "mode": "read", "doer": "plain", "seat": "d" }), Some(&[s.clone()])).unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_str()), ("plain", ""));
    let b = taken(|b| *b = json!({ "id": "one", "goal": "g", "done": "d", "mode": "read", "doer": "plain", "model": "big", "seat": "d" }), Some(&[s])).unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_str()), ("plain", "big"));
}

// ---- the brief's field ----

#[test]
fn a_seat_the_crew_does_not_have_is_refused_and_so_is_the_envoy_and_with_no_seats_the_field_is_not_read() {
    let crew = Crew::shipped();
    let named = |s: &str| taken(|b| b["seat"] = json!(s), Some(&crew.seats));
    let why = named("barista").unwrap_err();
    assert!(why.contains("`barista` is not a seat") && why.contains("reviewer") && !why.contains("envoy"), "{why}");
    assert!(named("envoy").unwrap_err().contains("operator's to open"));
    // `crew.on` off hands no seats over: the same answer is read as it was before seats were
    let with = taken(|b| b["seat"] = json!("reviewer"), None).unwrap();
    let without = taken(|_| {}, None).unwrap();
    assert_eq!(with, without);
    assert!(with.seat.is_none());
}

#[test]
fn a_brief_with_no_seat_comes_out_the_same_whether_the_crew_is_on_or_not() {
    let crew = Crew::shipped();
    let (on, off) = (taken(|_| {}, Some(&crew.seats)).unwrap(), taken(|_| {}, None).unwrap());
    assert_eq!(on, off);
    assert!(on.seat.is_none());
}

#[test]
fn the_six_shipped_seats_read_within_the_limit_and_a_seat_that_only_reads_is_one_of_four() {
    let crew = Crew::shipped();
    let ids: Vec<&str> = crew.seats.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["writer", "reviewer", "scout", "tester", "critic", "envoy"]);
    for s in crew.seats.iter() {
        assert!(s.check(1200).is_ok(), "{s:?}");
        assert!(!s.personality.is_empty() && s.personality.chars().count() <= 1200, "{}: {} characters", s.id, s.personality.chars().count());
        assert_eq!(s.id, s.role);
        assert!(s.tools_allow.is_empty() && s.tools_deny.is_empty() && s.paths.is_empty(), "the shipped seats cut no path and no tool");
    }
    let reads: Vec<&str> = crew.seats.iter().filter(|s| s.mode.as_deref() == Some("read")).map(|s| s.id.as_str()).collect();
    assert_eq!(reads, ["reviewer", "scout", "critic", "envoy"]);
    // a longer text than the setting allows is a seat that is left out
    let mut long = seat("long", None, &[]);
    long.personality = "x".repeat(1201);
    assert!(long.check(1200).unwrap_err().contains("crew.voice_max"));
    assert!(long.check(1201).is_ok());
}

// ---- through the front ----

fn rig(plan: &str, crew: bool) -> (Fx, Arc<Held>) {
    let held = Held::new();
    let doers = vec![chatty("claude", &[plan, "main worked"]), doer("pi", &["chat one", "chat two", "chat three"]), chatty("plain", &["plain chat"]), chatty("cheap", &[LOOK])];
    let f = fx_placed(doers, None, vec![held.place()], |c: &mut Cfg| {
        settings(c);
        if crew {
            c.crew = Crew::shipped();
        }
    });
    (f, held)
}

fn seated(id: &str, seat: Option<&str>, write: bool, writes: &[&str], doer: &str) -> Value {
    let mut b = brief(id, write, &A, writes);
    b["doer"] = json!(doer);
    if let Some(s) = seat {
        b["seat"] = json!(s);
    }
    b
}

/// Everything a chat task was given in words: its goal, its Brief and the text after the goal.
fn given(r: &j4n_core::task::Row) -> String {
    format!("{}\n{}\n{}", r.spec.goal, r.spec.input["brief"].as_str().unwrap_or_default(), r.spec.input["reports"].as_str().unwrap_or_default()) + r.spec.input["cli_text"].as_str().unwrap_or_default()
}

#[test]
fn a_chat_is_given_the_words_of_its_seat_as_a_section_of_its_own_and_the_log_says_what_the_seat_did() {
    // a reviewer seat on a brief that writes: the chat reads, the words reach a doer with a Brief and one without, and `chat.seated` is written
    let plan = answer(vec![seated("rev", Some("reviewer"), true, &["src/a/1.rs"], "pi"), seated("scan", Some("scout"), false, &[], "plain")]);
    let (f, _) = rig(&plan, true);
    say(&f);
    settle(&f);
    let chats = chat_tasks(&f);
    assert_eq!(chats.len(), 2, "{}", f.rig.dump());
    let reviewer = &Crew::shipped().seats[1];
    let (rev, scan) = (chats.iter().find(|r| r.spec.doer == "pi").unwrap(), chats.iter().find(|r| r.spec.doer == "plain").unwrap());
    assert_eq!((rev.spec.tune.mode, rev.spec.input["chat_write"].clone()), (Mode::Read, json!(false)), "{rev:?}");
    assert!(rev.spec.input["brief"].as_str().unwrap().contains(&reviewer.personality), "the doer with a Brief reads the seat's words: {}", given(rev));
    assert!(given(scan).contains(&Crew::shipped().seats[2].personality), "the doer with no Brief reads them after its goal: {}", given(scan));
    assert!(rev.spec.goal.contains("Your report holds: Findings, most serious first"), "{}", rev.spec.goal);
    let row = events(&f, "chat.seated");
    assert_eq!(row.len(), 2, "{row:?}");
    let row = row.iter().find(|r| r["seat"] == "reviewer").unwrap();
    assert_eq!((row["role"].as_str(), row["narrowed"].clone(), row["tools_allow"].clone()), (Some("reviewer"), json!(["mode: it only reads"]), json!([])), "{row}");
    // the brief row says what the chat was started as: a read chat, no write paths
    let briefed = events(&f, "chat.briefed");
    assert!(briefed.iter().any(|b| b["id"] == "rev" && b["mode"] == "read" && b["writes"] == json!([])), "{briefed:?}");
    assert!(f.notes().iter().any(|n| n.starts_with("brief · reads · pi") && n.contains("· seat reviewer")), "{:?}", f.notes());
    // a shipped seat names no tools: it narrows nothing and says nothing about the tool lists
    assert!(!f.notes().iter().any(|n| n.contains("`tools.")), "{:?}", f.notes());
}

#[test]
fn with_the_crew_off_a_brief_that_names_a_seat_starts_the_chat_exactly_as_one_that_does_not() {
    let (named, unnamed) = (answer(vec![seated("rev", Some("reviewer"), true, &["src/a/1.rs"], "pi")]), answer(vec![seated("rev", None, true, &["src/a/1.rs"], "pi")]));
    let (f, g) = (rig(&named, false).0, rig(&unnamed, false).0);
    let (a, b) = (say(&f), say(&g));
    settle(&f);
    settle(&g);
    assert!(matches!((&a, &b), (Said::Asking { .. }, Said::Asking { .. })), "{a:?} {b:?}");
    // the chat writes, so it waits for the yes; after it both start the same chat
    for (f, said) in [(&f, a), (&g, b)] {
        let Said::Asking { ask } = said else { unreachable!() };
        f.front.reply("main", &ask, j4n_proto::Reply::Yes);
        settle(f);
    }
    assert_eq!(events(&f, "chat.seated").len() + events(&g, "chat.seated").len(), 0);
    let row = |f: &Fx| events(f, "chat.briefed").iter().map(|e| (e["mode"].clone(), e["writes"].clone(), e["doer"].clone(), e["goal"].clone())).collect::<Vec<_>>();
    assert_eq!((row(&f), row(&f).len()), (row(&g), 1));
    let (x, y) = (chat_tasks(&f), chat_tasks(&g));
    assert_eq!(x.len(), 1);
    assert!(x.iter().zip(&y).all(|(x, y)| x.spec.goal == y.spec.goal && x.spec.tune == y.spec.tune && x.spec.input["brief"] == y.spec.input["brief"] && x.spec.input.get("seat").is_none()), "{x:?}");
    assert_eq!(x[0].spec.tune.mode, Mode::Write);
    // and the planning turn's prompt does not mention seats
    let prompt = |f: &Fx| f.tasks().into_iter().find(|r| r.spec.role.as_deref() == Some("split")).map(|r| r.spec.goal).unwrap_or_default();
    assert!(!prompt(&f).to_lowercase().contains("seat"), "{}", prompt(&f));
}

#[test]
fn the_planning_prompt_names_the_seats_while_the_crew_is_on_and_not_the_envoy() {
    let plan = answer(vec![seated("rev", Some("scout"), false, &[], "pi")]);
    let (f, _) = rig(&plan, true);
    say(&f);
    settle(&f);
    let prompt = f.tasks().into_iter().find(|r| r.spec.role.as_deref() == Some("split")).map(|r| r.spec.goal).unwrap_or_default();
    assert!(prompt.contains("\"seat\": \"a seat id, or leave it out\"") && prompt.contains("- reviewer: Reads a change") && prompt.contains("- scout: ") && !prompt.contains("- envoy:"), "{prompt}");
    assert!(prompt.contains(LINE));
}

#[test]
fn a_brief_whose_paths_lie_outside_its_seats_is_refused_whole_and_main_works_the_line() {
    let mut s = seat("docs-only", None, &["docs"]);
    s.doer = "pi".into();
    let plan = answer(vec![{
        let mut b = brief("one", false, &B, &[]);
        b["seat"] = json!("docs-only");
        b
    }]);
    let held = Held::new();
    let f = fx_placed(vec![chatty("claude", &[&plan, "main worked"]), doer("pi", &["chat one"]), chatty("cheap", &[LOOK])], None, vec![held.place()], |c: &mut Cfg| {
        settings(c);
        c.crew = Crew { seats: Arc::new(vec![s]), ..Crew::shipped() };
    });
    say(&f);
    settle(&f);
    assert!(chat_tasks(&f).is_empty() && f.notes().iter().any(|n| n.starts_with("no split:") && n.contains("none of its paths")), "{:?}", f.notes());
}

#[test]
fn a_brief_whose_goal_says_its_interfaces_fits_and_one_past_the_limit_is_refused_with_its_length() {
    // a brief that says its interface can run past 1500 characters; at 1200 the whole split would be dropped
    let long = |n: usize| move |b: &mut Value| b["goal"] = json!("x".repeat(n));
    assert!(taken(long(1513), None).is_ok());
    assert!(taken(long(2400), None).is_ok());
    let why = taken(long(2401), None).unwrap_err();
    assert!(why.contains("the goal is 2401 characters; one to 2400"), "{why}");
}
