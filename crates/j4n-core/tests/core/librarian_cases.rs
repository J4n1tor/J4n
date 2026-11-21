//! The librarian makes a task's context for that task and that doer, behind `librarian.on`, off by default. The loop,
//! the front, the Log and the ask point `librarian` are the real ones, the doers are scripted and Jev is the fixture's decider (it answers the pick it was
//! given, or is not there). What is asserted is what the doer was given, what the Log says, and that with the switch off nothing is different and nothing
//! is asked.

use crate::front_cases::{Fx, fx, fx_placed_on};
use crate::scripted::{Beat, Scripted, end, say};
use j4n_core::doer::Caps;
use j4n_core::front::librarian::{Shelf, TELL, TELL_CLI};
use j4n_core::front::session::Life;
use j4n_core::front::{Cfg, ProjectFacts, Said};
use j4n_core::skill::{Library, Skill, parse};
use j4n_core::task::Mode;
use serde_json::Value;
use std::sync::Arc;

const CARD: &str = "Project card, derived from the files:\nCargo.toml: rust";

struct Card;

impl ProjectFacts for Card {
    fn text(&self) -> Option<String> {
        Some(CARD.into())
    }
}

fn skill(name: &str, when: &str, cost: usize, body: &str) -> Skill {
    parse("pack:skills", name, &format!("---\nname: {name}\nwhen: {when}\ncost: {cost}\n---\n{body}\n")).unwrap()
}

/// Three skills that fit "build" alike, and one that fits tests.
fn library() -> Arc<Library> {
    let mut skills: Vec<Skill> = (0..3).map(|n| skill(&format!("s{n}"), "build", 100 + n * 10, &format!("# s{n}\nhow to build {n}"))).collect();
    skills.push(skill("checks", "tests", 200, "# Run the checks\nUse scripts/t."));
    Arc::new(Library::new(skills, ""))
}

fn model() -> Arc<Scripted> {
    let turns = (0..4).map(|_| vec![Beat::Session("claude-s".into()), say("ok"), end("ok")]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, brief: true, ..Caps::default() }, turns)
}

fn cli() -> Arc<Scripted> {
    let turns = (0..4).map(|_| vec![Beat::Session("cli-s".to_string()), say("ok"), end("ok")]).collect();
    Scripted::with_caps("claude", Caps { live: true, resume: true, skills: true, ..Caps::default() }, turns)
}

/// The doer the front says can ask, the card, the library, the mode; `on` is the switch.
fn set(on: bool, asks: bool, mode: Mode) -> impl FnOnce(&mut Cfg) {
    move |c| {
        c.doer = Some("claude".into());
        c.card = Some(Arc::new(Card));
        c.skills = Some(library());
        c.mode = mode;
        c.librarian.on = on;
        c.librarian.asks = if asks { vec!["claude".into()] } else { Vec::new() };
    }
}

fn brief(f: &Fx, n: usize) -> String {
    f.tasks()[n].spec.input.get("brief").and_then(Value::as_str).unwrap_or_default().to_string()
}

fn screen_with(max: f64) -> j4n_core::front::screen::Screen {
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.set = j4n_core::setting::Settings::from_pairs([("skills.max", j4n_core::value::Value::Num(max))]).tree();
    screen
}

/// How often Jev was put the point `librarian` (the first line of a thread is also put `intake`, which is not this).
fn put(f: &Fx) -> usize {
    f.asked.lock().unwrap().iter().filter(|q| q.point == "librarian").count()
}

fn events(f: &Fx, kind: &str) -> Vec<Value> {
    f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

#[test]
fn with_the_switch_off_the_brief_is_what_it_was_nothing_is_asked_and_nothing_is_written() {
    // the librarian's state is set up as the wiring would (a doer that can ask, a tie among skills, Jev there) and the switch is off
    let plain = fx(vec![model()], set(false, false, Mode::Write));
    let off = fx_placed_on(vec![model()], Some("librarian=s2"), Vec::new(), screen_with(2.0), set(false, true, Mode::Write));
    let before = fx_placed_on(vec![model()], None, Vec::new(), screen_with(2.0), set(false, false, Mode::Write));
    for f in [&plain, &off, &before] {
        f.turn("ping");
        f.turn("build it");
    }
    assert_eq!(brief(&off, 0), brief(&plain, 0), "a short line: the card is in it, as it always was");
    assert!(brief(&off, 0).contains("## project") && !brief(&off, 0).contains("`context`"), "{}", brief(&off, 0));
    assert_eq!(brief(&off, 1), brief(&before, 1), "a tie among skills: code's order, byte for byte");
    for f in [&plain, &off] {
        for kind in ["brief.picked", "brief.added"] {
            assert!(f.rig.of_kind(kind).is_empty(), "{kind}");
        }
        assert!(f.tasks().iter().all(|r| r.spec.input.get("held").is_none() && r.spec.input.get("context").is_none()));
    }
    assert_eq!(put(&off), 0, "Jev was not asked the point: not at a tie, not at all");
    assert!(off.front.asks().points.contains_key("librarian"), "(the point is there in this rig: the wiring takes it out while the switch is off)");
}

#[test]
fn code_picks_the_card_for_the_task_a_short_line_or_a_plain_question_has_none_and_the_doer_is_told_it_may_ask() {
    let cases = [
        ("ping", Mode::Write, false, "the line is short"),
        ("what does this do really", Mode::Read, false, "a plain question that names no file"),
        ("what does src/main.rs do", Mode::Read, true, ""),
        ("what is this project for", Mode::Read, true, ""),
        ("fix the parser", Mode::Write, true, ""),
    ];
    for (line, mode, card, why) in cases {
        let f = fx(vec![model()], set(true, true, mode));
        f.turn(line);
        let b = brief(&f, 0);
        assert_eq!(b.contains("## project"), card, "{line}: {b}");
        assert!(b.contains(TELL), "a doer that can ask is told so, in its contract: {b}");
        let row = &events(&f, "brief.picked")[0];
        assert_eq!(row["for"], "brief");
        if card {
            assert_eq!(f.tasks()[0].spec.input["held"], serde_json::json!([]), "{line}");
        } else {
            assert_eq!(f.tasks()[0].spec.input["held"], serde_json::json!(["project"]));
            assert_eq!(row["held"][0], serde_json::json!({ "what": "project", "why": why }), "{line}");
            assert!(row["line"].as_str().unwrap().contains(&format!("project held back ({why})")), "{row}");
        }
        assert!(f.notes().iter().any(|n| n == row["line"].as_str().unwrap()), "the chat says what the Log says: {:?}", f.notes());
    }
    // a doer that cannot ask has nothing held back, and is not told it may
    let f = fx(vec![model()], set(true, false, Mode::Read));
    f.turn("ping");
    assert!(brief(&f, 0).contains("## project") && !brief(&f, 0).contains("`context`"), "{}", brief(&f, 0));
    assert_eq!(f.tasks()[0].spec.input["held"], serde_json::json!([]));
    // the next line of a going session carries no new contract line and writes no row of its own
    let f = fx(vec![model()], set(true, true, Mode::Write));
    f.turn("fix the parser");
    f.turn("and add a test for it");
    assert!(!brief(&f, 1).contains("`context`"), "{}", brief(&f, 1));
    assert_eq!(events(&f, "brief.picked").len(), 1);
}

#[test]
fn where_skills_fit_alike_jev_chooses_and_where_jev_is_absent_or_has_no_better_one_code_s_pick_stands_and_the_log_says_so() {
    let ids = |f: &Fx| -> Vec<String> { f.tasks()[0].spec.input["skills"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap().to_string()).collect() };
    let run = |pick: Option<&'static str>| {
        let f = fx_placed_on(vec![model()], pick, Vec::new(), screen_with(2.0), set(true, true, Mode::Write));
        f.turn("build it");
        f
    };
    // Jev names one of the three: it is taken first, and the other place is code's
    let jev = run(Some("librarian=s2"));
    assert_eq!(ids(&jev), ["s2", "s0"]);
    assert_eq!(put(&jev), 1, "asked once for the task");
    let q = jev.asked.lock().unwrap().iter().find(|q| q.point == "librarian").cloned().expect("the point was put");
    assert_eq!(q.point, "librarian");
    let j4n_core::ask::AskShape::Choice { options } = &q.shape else { panic!("a choice") };
    assert_eq!(options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["s0", "s1", "s2", "none"]);
    assert!(options[2].label.starts_with("s2: ") && options[2].label.contains("s2"), "{options:?}");
    let row = &events(&jev, "brief.picked")[0];
    assert!(row["jev"].as_str().unwrap().starts_with("asked (decider:jev): s2"), "{row}");
    // Jev says none is better: code's order stands, and the row says Jev was asked
    let none = run(Some("librarian=none"));
    assert_eq!(ids(&none), ["s0", "s1"]);
    assert!(events(&none, "brief.picked")[0]["jev"].as_str().unwrap().contains("none better; the pick of code stands"));
    // Jev is not there: code's pick stands and the Log says so
    let absent = run(Some("absent"));
    assert_eq!(ids(&absent), ["s0", "s1"]);
    assert_eq!(put(&absent), 0);
    let row = &events(&absent, "brief.picked")[0];
    assert!(row["jev"].as_str().unwrap().starts_with("absent: ") && row["jev"].as_str().unwrap().ends_with("the pick of code stands"), "{row}");
    assert!(row["line"].as_str().unwrap().contains("jev: absent"), "{row}");
    // where code can choose, Jev is not asked: one skill fits
    let f = fx_placed_on(vec![model()], Some("librarian=s2"), Vec::new(), screen_with(2.0), set(true, true, Mode::Write));
    f.turn("run the tests");
    assert_eq!(put(&f), 0);
    assert_eq!(f.tasks()[0].spec.input["skills"][0]["id"], "checks");
}

#[test]
fn the_doer_asks_for_more_through_the_shelf_and_the_log_says_what_was_added_and_why() {
    let f = fx(vec![model()], set(true, true, Mode::Read));
    let shelf = Shelf::default();
    let front = f.front.clone().with_librarian(&shelf);
    let more = shelf.get().expect("the front answers");
    let task = match front.say("main", "ping") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    // what there is: the card (held back from a short line) and the skills that were not picked
    let menu = more.menu(&task);
    assert!(menu.starts_with("`context` can add") && menu.contains("project ·") && menu.contains("checks · tests · ~200 tokens") && menu.contains("s1 · build"), "{menu}");
    // the card, with the reason the doer gave: its text, and one `brief.added`
    assert_eq!(more.add(&task, "project", "need the layout to name the module").unwrap(), CARD);
    let added = events(&f, "brief.added");
    assert_eq!(added.len(), 1);
    assert_eq!((added[0]["task"].as_str(), added[0]["what"].as_str(), added[0]["why"].as_str(), added[0]["by"].as_str(), added[0]["doer"].as_str()), (Some(task.as_str()), Some("project"), Some("need the layout to name the module"), Some("doer"), Some("claude")));
    assert_eq!(added[0]["tokens"], j4n_core::brief::tokens(CARD));
    assert_eq!(added[0]["line"].as_str().unwrap(), format!("context · added project (~{} tokens) at the doer's ask: need the layout to name the module", j4n_core::brief::tokens(CARD)));
    assert!(f.notes().iter().any(|n| n == added[0]["line"].as_str().unwrap()), "{:?}", f.notes());
    // a skill by its name; what it already has is said and is not an addition; what there is not is said too
    assert!(more.add(&task, "checks", "to run them").unwrap().contains("Use scripts/t."));
    assert!(more.add(&task, "project", "").unwrap().contains("added to your context already"));
    assert!(more.add(&task, "no-such", "").unwrap_err().contains("nothing called `no-such`"));
    assert_eq!(events(&f, "brief.added").len(), 2, "only what was added is written");
    // the cap is the setting: three additions, and the fourth is refused with the reason
    assert!(more.add(&task, "s1", "x").is_ok());
    let e = more.add(&task, "s2", "x").unwrap_err();
    assert!(e.contains("as many as it may (`librarian.more`)"), "{e}");
    assert_eq!(events(&f, "brief.added").len(), 3);
    // the next line of the session is the same session's: it has its count, and what it was given
    let second = match front.say("main", "ping") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&second);
    assert!(more.add(&second, "project", "").unwrap().contains("added to your context already"));
    assert!(more.add(&second, "s2", "y").unwrap_err().contains("this session has asked"));
    // the list, and a want that says nothing, add nothing and are not counted: they still answer when the cap is reached
    for want in ["list", "", "  "] {
        let out = more.add(&second, want, "").unwrap();
        assert!(out.starts_with("`context` can add") && !out.contains("project ·") && out.contains("s2 · build"), "{want:?}: {out}");
    }
    assert_eq!(events(&f, "brief.added").len(), 3);
}

#[test]
fn a_card_held_back_from_the_first_line_is_still_there_to_ask_for_in_a_later_line_of_the_session() {
    let f = fx(vec![model()], set(true, true, Mode::Write));
    let shelf = Shelf::default();
    let front = f.front.clone().with_librarian(&shelf);
    let more = shelf.get().unwrap();
    let say = |line: &str| match front.say("main", line) {
        Said::Started { task } => {
            f.rig.finished(&task);
            task
        }
        other => panic!("{other:?}"),
    };
    let first = say("ping");
    assert_eq!(f.tasks()[0].spec.input["held"], serde_json::json!(["project"]));
    let second = say("fix src/parser.rs");
    assert_eq!(f.tasks()[1].spec.follows.as_deref(), Some(first.as_str()), "the same session");
    assert_eq!(f.tasks()[1].spec.input["held"], serde_json::json!([]), "this line was given what it was given");
    // the card was never in this session: the menu names it and the ask gets it
    assert!(more.menu(&second).contains("project ·"), "{}", more.menu(&second));
    assert_eq!(more.add(&second, "project", "now it is a file").unwrap(), CARD);
    assert_eq!(events(&f, "brief.added").len(), 1);
    assert!(!more.menu(&second).contains("project ·"), "and then it is not offered again");
}

#[test]
fn what_an_ask_returns_is_cut_to_the_tokens_the_session_may_add_and_the_answer_says_so_and_the_list_is_cut() {
    let f = fx(vec![model()], |c| {
        set(true, true, Mode::Write)(c);
        let mut skills: Vec<Skill> = vec![skill("big", "huge", 5000, &format!("# Big\n{}", "one more line of it\n".repeat(400)))];
        skills.extend((0..30).map(|n| skill(&format!("x{n:02}"), "never", 10, "# x\nwords")));
        c.skills = Some(Arc::new(Library::new(skills, "")));
    });
    let shelf = Shelf::default();
    let front = f.front.clone().with_librarian(&shelf);
    let more = shelf.get().unwrap();
    let task = match front.say("main", "ping") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    // the list shows twenty lines and says how many more there are
    let menu = more.menu(&task);
    assert_eq!(menu.lines().count(), 1 + 20 + 1, "{menu}");
    assert!(menu.ends_with("… and 12 more: ask by a name you know, or a word of one"), "{menu}");
    // a skill of ~2,000 tokens is cut at a line to what a task's skills may come to (1,200, the setting's default), and the answer and the Log say so
    let got = more.add(&task, "big", "to read it all").unwrap();
    assert!(got.contains("[cut (cut to ~") && got.contains("`skills.tokens`"), "{got}");
    let body = got.split("\n[cut").next().unwrap();
    assert!(j4n_core::brief::tokens(body) <= 1200 && body.ends_with("one more line of it"), "{}", j4n_core::brief::tokens(body));
    let added = &events(&f, "brief.added")[0];
    assert_eq!((added["cut"].clone(), added["what"].as_str()), (serde_json::json!(true), Some("big")));
    assert!(added["tokens"].as_u64().unwrap() <= 1200 && added["of"].as_u64().unwrap() > 1200, "{added}");
    assert!(added["line"].as_str().unwrap().contains(" of ~"), "{added}");
    // the session has taken what it may and no more: a later ask gets what is left of it, and the additions together stay within the cap
    let _ = more.add(&task, "x01", "");
    let total: u64 = events(&f, "brief.added").iter().map(|e| e["tokens"].as_u64().unwrap()).sum();
    assert!(total <= 1200, "{total}");
    assert!(more.menu(&task).starts_with("`context` can add"));
}

#[test]
fn a_task_that_follows_a_session_the_doer_already_has_the_card_in_is_told_it_has_it() {
    // the card was in the first Brief: asking for it is no addition
    let f = fx(vec![model()], set(true, true, Mode::Write));
    let shelf = Shelf::default();
    let front = f.front.clone().with_librarian(&shelf);
    let task = match front.say("main", "fix the parser") {
        Said::Started { task } => task,
        other => panic!("{other:?}"),
    };
    f.rig.finished(&task);
    assert!(shelf.get().unwrap().add(&task, "project", "").unwrap().contains("already in your context"));
    assert!(f.rig.of_kind("brief.added").is_empty());
}

#[test]
fn a_cli_doer_gets_its_carry_over_the_picked_skills_and_one_line_that_says_there_is_more() {
    let doer = cli();
    let f = fx(vec![doer.clone()], |c| {
        set(true, true, Mode::Write)(c);
        c.session = Life::Each;
    });
    f.turn("run the tests in the crate");
    let second = f.turn("run the tests again please");
    let input = &second.spec.input;
    // the carry-over of the session mode (a session of its own for each line) and the skills J4n picked, as a folder of their own, are as they were
    assert!(input["carry"].as_str().unwrap_or_default().contains("run the tests in the crate"), "{input}");
    assert_eq!(input["skills"][0]["id"], "checks");
    assert!(input["skills"][0]["body"].as_str().unwrap().contains("Use scripts/t."));
    // and the librarian adds the one line, and names what the card and the unpicked skills could add
    assert_eq!(input["context"], TELL_CLI);
    assert_eq!(input["held"], serde_json::json!(["project"]));
    assert!(input.get("brief").is_none(), "a doer that runs a program of its own is given no Brief");
    let told = f.tasks().iter().map(|r| r.spec.input.clone()).collect::<Vec<_>>();
    assert!(told.iter().all(|i| i["context"] == TELL_CLI));
    let rows = events(&f, "brief.picked");
    assert!(rows.len() >= 2 && rows.iter().all(|r| r["for"] == "cli"), "{rows:?}");
    // the engine puts it after the goal, with the carry-over, in the first text of the session
    let starts = doer.starts();
    let text = starts[1].text.clone().unwrap_or_default();
    assert!(text.starts_with("run the tests again please\n\n") && text.contains("Carried over") && text.ends_with(TELL_CLI), "{text}");
    // with the switch off the same doer is given what it was: no line, nothing held
    let plain = cli();
    let off = fx(vec![plain.clone()], |c| {
        set(false, true, Mode::Write)(c);
        c.session = Life::Each;
    });
    off.turn("run the tests in the crate");
    let again = off.turn("run the tests again please");
    assert!(again.spec.input.get("context").is_none() && again.spec.input.get("held").is_none() && off.rig.of_kind("brief.picked").is_empty());
    assert!(!plain.starts()[1].text.clone().unwrap_or_default().contains("j4n.call"));
}

#[test]
fn the_line_a_cli_doer_is_told_is_told_once_and_a_task_that_is_resumed_is_not_told_it_again() {
    let doer = cli();
    let f = fx(vec![doer.clone()], set(true, true, Mode::Write));
    let first = f.turn("run the tests in the crate");
    assert!(doer.starts()[0].text.clone().unwrap_or_default().ends_with(TELL_CLI), "{:?}", doer.starts()[0].text);
    assert!(first.spec.input.get("context").is_some());
    let next = f.rig.tasks.resume(&first.id, Some("go on"), j4n_core::change::Actor::Operator).unwrap();
    f.rig.finished(&next);
    assert_eq!(doer.starts()[1].text, None, "the turn that carries on in the session is given its goal and nothing the session was told: {:?}", doer.starts()[1].text);
}

#[test]
fn a_skill_chosen_by_jev_that_does_not_fit_the_token_cap_is_left_out_and_code_s_order_fills_the_places() {
    let lib = Library::new(vec![skill("s0", "build", 100, "# s0\nb"), skill("s1", "build", 110, "# s1\nb"), skill("s2", "build", 5000, "# s2\nb")], "");
    let ask = |tokens: usize, max: usize| j4n_core::skill::Ask { line: "build it", files: Vec::new(), kinds: Vec::new(), in_session: false, max, tokens };
    let ids = |l: Vec<j4n_core::skill::Picked>| l.into_iter().map(|p| p.id).collect::<Vec<_>>();
    // the three tie, there is room for two: Jev is asked, and names the one that is too big for the cap
    let mut asked = Vec::new();
    let got = lib.pick_by(&ask(250, 2), &mut |g| {
        asked.push(g.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
        Some("s2".into())
    });
    assert_eq!(asked, [["s0", "s1", "s2"]]);
    assert_eq!(ids(got), ["s0", "s1"], "the skill that does not fit is left out; the others keep code's order");
    // one that fits goes first, and the other place is code's
    assert_eq!(ids(lib.pick_by(&ask(250, 2), &mut |_| Some("s1".into()))), ["s1", "s0"]);
    // a name that is not one of them, or none, is code's order; and where code can choose (room for all three) Jev is not asked
    assert_eq!(ids(lib.pick_by(&ask(250, 2), &mut |_| Some("zz".into()))), ["s0", "s1"]);
    assert_eq!(ids(lib.pick_by(&ask(9000, 3), &mut |_| panic!("code can choose"))), ["s0", "s1", "s2"]);
}
