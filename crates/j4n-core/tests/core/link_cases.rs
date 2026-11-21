//! Links: who may talk to whom. The star is today's routing and nothing else; the declared rows carry only what they name, a thread passes a row as many times
//! as its `hops` say, a cycle that no `hops` caps is refused when the rows load, and a message no row carries is refused with the reason. Then the same through the
//! real front at each place a message passes (a brief, a report, an opinion, an envoy's request, a question of the work, Main's report to the envoy), and what
//! Main decided on the opinions it had.

use crate::branch_validate::{A, CLEAN, Held, Rig3, checker, doer};
use crate::front_cases::{Fx, chatty, fx, fx_placed};
use crate::scripted::{Beat, Scripted, approval, end};
use crate::world::{layer, refused};
use j4n_core::branch::Switch;
use j4n_core::change::Actor;
use j4n_core::crew::Crew;
use j4n_core::front::{Cfg, Said};
use j4n_core::links::{CARRIES, DEFAULT_HOPS, Link, Links, Party, REQUEST_HOPS, Refused};
use j4n_core::log::New;
use j4n_core::table::{Level, LayerKind, Problem, Tables};
use j4n_core::task::{State, TaskSpec};
use serde_json::{Value, json};
use std::sync::Arc;

// ---- the rows ----

const KIND: &str = include_str!("../../../../packs/crew/kinds/link.toml");

fn chat(id: &str, seat: Option<&str>) -> Party {
    Party::Chat { id: id.into(), seat: seat.map(String::from) }
}

fn envoy(id: &str) -> Party {
    Party::Envoy(id.into())
}

/// The set the rows say, read as the front reads them: the kind from the pack and the rows from the operator's own file.
fn load(rows: &str, declared: bool) -> (Links, Vec<Problem>) {
    let (t, p) = Tables::load(vec![layer(LayerKind::Pack, "crew", &[("kinds/link.toml", KIND)]), layer(LayerKind::User, "home", &[("links.toml", rows)])]);
    assert!(refused(&p).is_empty(), "the rows read: {p:?}");
    Links::load(&t, declared, &["reviewer".to_string(), "scout".to_string(), "envoy".to_string()])
}

fn faults(p: &[Problem]) -> Vec<String> {
    p.iter().filter(|p| p.level == Level::Refused).map(|p| format!("{}: {}", p.at, p.msg)).collect()
}

/// The ids of the rows, in the order the Table gives them (by id).
fn ids(l: &Links) -> Vec<&str> {
    let mut v: Vec<&str> = l.rows().iter().map(|r| r.id.as_str()).collect();
    v.sort_unstable();
    v
}

const NEVER: &dyn Fn(&str) -> usize = &|_| 0;

#[test]
fn the_star_is_todays_routing_and_every_other_way_is_refused_with_the_reason() {
    let star = Links::star();
    let (main, c1, e4) = (Party::Main, chat("c1", Some("reviewer")), envoy("c4"));
    for (from, to, what) in [(&e4, &main, "request"), (&main, &c1, "brief"), (&c1, &main, "report"), (&c1, &main, "opinion"), (&main, &e4, "report"), (&c1, &e4, "question"), (&main, &e4, "question")] {
        assert!(star.admit(from, to, what, NEVER).is_ok(), "{} {what} {}", from.name(), to.name());
    }
    for (from, to, what) in [(&c1, &main, "request"), (&e4, &c1, "brief"), (&c1, &c1, "report"), (&main, &c1, "opinion"), (&c1, &e4, "request"), (&e4, &main, "brief"), (&main, &c1, "report"), (&e4, &main, "opinion"), (&c1, &main, "brief")] {
        let why = star.admit(from, to, what, NEVER).unwrap_err();
        assert_eq!(why, Refused::Undeclared(format!("no link carries a {what} from {} to {}", from.name(), to.name())));
    }
    // loaded as the setting says, `star` is the same set, whatever rows the operator wrote
    let (loaded, problems) = load("[[link]]\nid = \"x\"\nfrom = \"main\"\nto = \"chat\"\ncarries = [\"note\"]\n", false);
    assert!(problems.is_empty() && !loaded.declared() && ids(&loaded) == ids(&star), "{problems:?}");
}

#[test]
fn the_envoys_request_is_held_to_three_and_the_rest_of_the_star_to_what_holds_it_today() {
    let star = Links::star();
    let (e, main) = (envoy("c4"), Party::Main);
    assert_eq!(REQUEST_HOPS, 3);
    for used in 0..3 {
        assert!(star.admit(&e, &main, "request", &|_| used).is_ok(), "{used}");
    }
    assert_eq!(star.admit(&e, &main, "request", &|_| 3).unwrap_err(), Refused::Hops { link: "star-request".into(), cap: 3 });
    // a brief, a report and a question have no cap of their own in the star: the chats Main may start, the plan's rounds and the asks are theirs
    for (from, to, what) in [(&main, &chat("c1", None), "brief"), (&chat("c1", None), &main, "report"), (&chat("c1", None), &e, "question")] {
        assert!(star.admit(from, to, what, &|_| 1000).is_ok(), "{what}");
    }
}

#[test]
fn declared_rows_carry_what_they_name_and_nothing_else() {
    let (links, problems) = load("[[link]]\nid = \"back\"\nfrom = \"chat\"\nto = \"main\"\ncarries = [\"report\"]\n\n[[link]]\nid = \"look\"\nfrom = \"reviewer\"\nto = \"main\"\ncarries = [\"opinion\"]\nhops = 2\n", true);
    assert!(faults(&problems).is_empty() && links.declared() && ids(&links) == ["back", "look"], "{problems:?}");
    let main = Party::Main;
    assert!(links.admit(&chat("c1", None), &main, "report", NEVER).is_ok());
    assert!(links.admit(&chat("c1", None), &main, "opinion", NEVER).is_err(), "a chat with no seat is not the reviewer");
    assert!(links.admit(&chat("c1", Some("reviewer")), &main, "opinion", NEVER).is_ok(), "a seat names the chat that sits in it");
    assert!(links.admit(&chat("c1", Some("scout")), &main, "opinion", NEVER).is_err());
    assert!(links.admit(&main, &chat("c1", None), "brief", NEVER).is_err(), "the star's brief is not there unless it is written");
    assert!(links.admit(&envoy("c4"), &main, "request", NEVER).is_err());
    // an endpoint that names one chat or one envoy names only that
    let (one, _) = load("[[link]]\nid = \"one\"\nfrom = \"envoy:c4\"\nto = \"main\"\ncarries = [\"request\"]\n\n[[link]]\nid = \"two\"\nfrom = \"main\"\nto = \"c2\"\ncarries = [\"brief\"]\n", true);
    assert!(one.admit(&envoy("c4"), &main, "request", NEVER).is_ok() && one.admit(&envoy("c5"), &main, "request", NEVER).is_err());
    assert!(one.admit(&main, &chat("c2", None), "brief", NEVER).is_ok() && one.admit(&main, &chat("c3", None), "brief", NEVER).is_err());
    // a declared set with no row that holds refuses everything, and says so
    let (none, problems) = load("", true);
    assert!(none.rows().is_empty() && problems.iter().any(|p| p.level == Level::Warn && p.msg.contains("every message is refused")), "{problems:?}");
}

#[test]
fn a_rows_hops_are_how_many_times_a_thread_may_pass_it_three_when_left_out_and_none_for_zero() {
    let rows = |hops: &str| format!("[[link]]\nid = \"r\"\nfrom = \"chat\"\nto = \"main\"\ncarries = [\"report\"]\n{hops}");
    let pass = |links: &Links, used: usize| links.admit(&chat("c1", None), &Party::Main, "report", &|_| used).is_ok();
    let (left_out, _) = load(&rows(""), true);
    assert_eq!(DEFAULT_HOPS, 3);
    assert!(pass(&left_out, 2) && !pass(&left_out, 3));
    let (two, _) = load(&rows("hops = 2\n"), true);
    assert!(pass(&two, 1) && !pass(&two, 2));
    assert_eq!(two.admit(&chat("c1", None), &Party::Main, "report", &|_| 2).unwrap_err().to_string(), "link `r` has been passed 2 times in this thread: it stops here, ask the operator before another");
    let (none, _) = load(&rows("hops = 0\n"), true);
    assert!(pass(&none, 1_000_000), "0 is no cap");
    // two rows carry it: it goes by the one that has room
    let (both, _) = load(&format!("{}\n[[link]]\nid = \"s\"\nfrom = \"any\"\nto = \"main\"\ncarries = [\"report\"]\nhops = 5\n", rows("hops = 1\n")), true);
    assert!(pass(&both, 1) && pass(&both, 4) && !pass(&both, 5));
}

#[test]
fn a_cycle_of_rows_that_no_hops_caps_is_refused_when_they_load_and_a_cap_anywhere_on_it_lets_it_stand() {
    let row = |id: &str, from: &str, to: &str, hops: &str| format!("[[link]]\nid = \"{id}\"\nfrom = \"{from}\"\nto = \"{to}\"\ncarries = [\"report\"]\n{hops}\n");
    // Main briefs a chat and the chat reports to Main, neither capped
    let (links, problems) = load(&format!("{}{}", row("go", "main", "chat", "hops = 0"), row("back", "chat", "main", "hops = 0")), true);
    let f = faults(&problems);
    assert!(ids(&links).is_empty() && f.len() == 2 && f.iter().all(|m| m.contains("is on a cycle that no `hops` caps")), "{f:?}");
    // one of them capped (left out is three): the cycle is capped and both stand
    let (links, problems) = load(&format!("{}{}", row("go", "main", "chat", "hops = 0"), row("back", "chat", "main", "")), true);
    assert!(faults(&problems).is_empty() && ids(&links) == ["back", "go"], "{problems:?}");
    // a row that follows itself
    let (links, problems) = load(&row("loop", "chat", "chat", "hops = 0"), true);
    assert!(ids(&links).is_empty() && faults(&problems).len() == 1);
    // the cycle closes only through a word that names everyone
    let (links, problems) = load(&format!("{}{}", row("a", "main", "c4", "hops = 0"), row("b", "any", "main", "hops = 0")), true);
    assert!(ids(&links).is_empty() && faults(&problems).len() == 2, "{problems:?}");
    // an uncapped row on no cycle, and rows of two chats that never meet, stand
    let (links, problems) = load(&format!("{}{}{}", row("one-way", "main", "chat", "hops = 0"), row("c4-c5", "c4", "c5", "hops = 0"), row("c4-c6", "c4", "c6", "hops = 0")), true);
    assert!(faults(&problems).is_empty() && ids(&links) == ["c4-c5", "c4-c6", "one-way"], "{problems:?}");
    // a row on a cycle is the only one left out: a capped row that leads into the cycle stays
    let (links, problems) = load(&format!("{}{}{}", row("go", "main", "chat", "hops = 0"), row("back", "chat", "main", "hops = 0"), row("up", "envoy", "main", "")), true);
    assert!(ids(&links) == ["up"] && faults(&problems).len() == 2, "{problems:?}");
}

#[test]
fn a_row_that_names_nobody_or_nothing_is_refused_at_load_with_the_word_that_is_wrong() {
    let rows = |from: &str, to: &str, carries: &str, extra: &str| format!("[[link]]\nid = \"x\"\nfrom = \"{from}\"\nto = \"{to}\"\ncarries = {carries}\n{extra}\n");
    for (text, wants) in [
        (rows("main", "ghost", "[\"brief\"]", ""), "`to`: `ghost` is not"),
        (rows("envoy:four", "main", "[\"request\"]", ""), "`envoy:` is followed by"),
        (rows("main", "chat", "[\"shout\"]", ""), "`shout` is not something a link carries"),
        (rows("main", "chat", "[]", ""), "it carries nothing"),
        (rows("main", "chat", "[\"brief\"]", "hops = 101"), "`hops` is at most 100"),
    ] {
        let (links, problems) = load(&text, true);
        let f = faults(&problems);
        assert!(f.iter().any(|m| m.contains(wants)), "{wants}: {f:?}");
        assert!(links.rows().len() <= 1, "the row that is wrong is left out: {:?}", ids(&links));
    }
    // every word a link may carry is one the front passes
    assert_eq!(CARRIES, ["request", "brief", "report", "opinion", "question", "note"]);
}

#[test]
fn the_setting_reads_the_star_or_the_rows_and_the_crew_holds_the_set() {
    use j4n_core::setting::Settings;
    use j4n_core::value::Value;
    let (t, _) = Tables::load(vec![layer(LayerKind::Pack, "crew", &[("kinds/link.toml", KIND)]), layer(LayerKind::User, "home", &[("links.toml", "[[link]]\nid = \"only\"\nfrom = \"main\"\nto = \"chat\"\ncarries = [\"brief\"]\n")])]);
    let word = |w: &str| Settings::from_pairs([("crew.on", Value::Bool(true)), ("crew.links", Value::text(w))]);
    let (declared, _) = Crew::load(&t, &word("declared"));
    assert!(declared.links.declared() && ids(&declared.links) == ["only"]);
    let (star, _) = Crew::load(&t, &word("star"));
    assert!(!star.links.declared() && ids(&star.links) == ids(&Links::star()));
    let (unset, _) = Crew::load(&t, &Settings::default());
    assert!(!unset.links.declared(), "nothing set: the star");
    assert!(!Crew::shipped().links.declared());
}

// ---- the report's opinion ----

#[test]
fn a_report_may_hold_an_opinion_cut_to_four_hundred_tokens_and_a_report_without_one_reads_as_it_did() {
    use j4n_core::report::{OPINION_WHY, Report};
    let with = |extra: Value| {
        let mut o = json!({ "answer": "done", "found": [], "changed": "", "ran": [], "sure": "high", "open": [] });
        o.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        o.to_string()
    };
    let r = Report::take(&with(json!({ "opinion": { "pick": " keep the retry loop ", "why": "x".repeat(OPINION_WHY * 2) } }))).unwrap();
    let o = r.opinion.as_ref().expect("an opinion");
    assert_eq!(o.pick, "keep the retry loop");
    assert_eq!(o.why.chars().count(), OPINION_WHY + 1, "cut at 400 tokens, the cut marked");
    assert_eq!(OPINION_WHY, 400 * 4);
    // an empty pick, or no reason, is no opinion; the rest of the report stands
    for bad in [json!({ "opinion": { "pick": "", "why": "because" } }), json!({ "opinion": { "pick": "a", "why": "  " } })] {
        let r = Report::take(&with(bad)).unwrap();
        assert!(r.opinion.is_none() && r.answer == "done");
    }
    // none: the JSON of the report is what it was, with no `opinion` key
    let r = Report::take(&with(json!({}))).unwrap();
    assert!(r.opinion.is_none() && r.json().get("opinion").is_none(), "{}", r.json());
}

// ---- through the real front ----

const LINE: &str = "check the service and rewrite its docs";
const ONE: &str = r#"{"decision":"one","chats":[],"features":[],"why":"a question"}"#;

fn settings(c: &mut Cfg) {
    c.doer = Some("claude".into());
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.orchestrate.look = true;
    c.orchestrate.ask = "never".into();
    c.orchestrate.doer = "zcheap".into();
    c.crew = Crew::shipped();
}

fn brief() -> Value {
    json!({ "id": "part", "goal": "do the part and report", "paths": A, "writes": [], "done": "it is done", "doer": "pi", "model": "", "mode": "read", "features": [], "context": "brief", "checks": [] })
}

fn report(opinion: Option<(&str, &str)>) -> String {
    let mut r = json!({ "answer": "it is done", "found": [], "changed": "", "ran": [], "sure": "high", "open": [] });
    if let Some((pick, why)) = opinion {
        r["opinion"] = json!({ "pick": pick, "why": why });
    }
    r.to_string()
}

fn rows(spec: &[(&str, &str, &str, &[&str], u32)]) -> Arc<Links> {
    Arc::new(Links::of(spec.iter().map(|(id, from, to, carries, hops)| Link { id: id.to_string(), doc: None, from: from.to_string(), to: to.to_string(), carries: carries.iter().map(|c| c.to_string()).collect(), hops: Some(*hops) }).collect()))
}

/// Main plans one chat that reads; the chat answers with the next of `reports`; Main's later turns answer with the next of `main_says`.
fn flow(reports: &[&str], main_says: &[&str], tune: impl FnOnce(&mut Cfg)) -> (Rig3, Arc<Scripted>) {
    let held = Held::new();
    let plan = json!({ "chats": [brief()], "main": "" }).to_string();
    let mut says = vec![plan.as_str()];
    says.extend(main_says);
    let claude = doer("claude", &says);
    let look = json!({ "decision": "split", "chats": [{ "goal": "do the part", "writes": true, "doer": "pi", "model": "" }], "features": [], "why": "one part to hand off" }).to_string();
    let doers = vec![claude.clone(), doer("pi", reports), doer("own", &[CLEAN; 6]), checker(2), chatty("zcheap", &[&look, ONE])];
    let f: Fx = fx_placed(doers, Some("pi"), vec![held.place()], |c| {
        settings(c);
        tune(c);
    });
    (Rig3 { f, held }, claude)
}

fn events(r: &Rig3, kind: &str) -> Vec<Value> {
    r.f.rig.of_kind(kind).into_iter().map(|e| e.data).collect()
}

/// Every task is over, then the plan goes on, until `done`.
fn until(r: &Rig3, what: &str, done: impl Fn(&Rig3) -> bool) {
    r.until(what, done);
}

fn chat_text(r: &Rig3) -> String {
    r.f.tasks().into_iter().find(|t| t.spec.input.get("chat").is_some()).map(|t| t.spec.goal).unwrap_or_default()
}

#[test]
fn under_the_star_a_split_runs_as_it_did_and_the_log_holds_no_link_event() {
    let (r, _) = flow(&[&report(None)], &[], |_| {});
    assert!(matches!(r.f.front.say("main", LINE), Said::Nothing), "{}", r.f.rig.dump());
    until(&r, "the scout ended", |r| r.item("c1").done());
    let d = events(&r, "orchestrate.digest");
    assert!(d.len() == 1 && d[0]["text"].as_str().unwrap().starts_with("c1 do the part and report · scout reviewed") && d[0]["wake"] == false, "{d:?}");
    for kind in ["crew.sent", "crew.refused", "crew.opinion", "crew.decided"] {
        assert!(events(&r, kind).is_empty(), "{kind}: {}", r.f.rig.dump());
    }
    assert!(r.f.tasks().iter().all(|t| !t.spec.goal.starts_with("J4n:")), "Main was not woken");
}

#[test]
fn a_brief_no_link_carries_is_not_started_and_the_reason_is_in_the_log_and_in_main() {
    let (r, _) = flow(&[&report(None)], &[], |c| c.crew.links = rows(&[("back", "chat", "main", &["report"], 3)]));
    r.f.front.say("main", LINE);
    assert!(r.f.tasks().iter().all(|t| t.spec.input.get("chat").is_none()), "no chat started: {}", r.f.rig.dump());
    let refused = events(&r, "crew.refused");
    assert!(refused.len() == 1 && refused[0]["carries"] == "brief" && refused[0]["from"] == "main" && refused[0]["why"].as_str().unwrap().starts_with("no link carries a brief from main to"), "{refused:?}");
    assert!(r.f.notes().iter().any(|n| n.contains("was not started: no link carries a brief")), "{:?}", r.f.notes());
    // the seat the brief names is what the row is read against: a row for the reviewer does not carry a brief to a chat with none
    let (r, _) = flow(&[&report(None)], &[], |c| c.crew.links = rows(&[("brief", "main", "reviewer", &["brief"], 0)]));
    r.f.front.say("main", LINE);
    assert_eq!(events(&r, "crew.refused").len(), 1, "{}", r.f.rig.dump());
}

#[test]
fn a_report_no_link_carries_is_not_given_to_main_and_the_operator_is_told_once() {
    let (r, _) = flow(&[&report(None)], &[], |c| c.crew.links = rows(&[("brief", "main", "chat", &["brief"], 0)]));
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    for _ in 0..3 {
        r.step();
    }
    assert!(events(&r, "orchestrate.digest").is_empty(), "{}", r.f.rig.dump());
    let refused = events(&r, "crew.refused");
    assert!(refused.len() == 1 && refused[0]["carries"] == "report" && refused[0]["from"] == "c1", "once, not at every step: {refused:?}");
    assert!(r.f.notes().iter().any(|n| n.contains("a report from c1 to main was not passed")), "{:?}", r.f.notes());
    // the plan still ran: what was refused is what Main is told, not what the chat did
    assert_eq!(r.item("c1").state, "reviewed");
}

#[test]
fn a_thread_passes_a_report_link_as_often_as_its_hops_say_and_then_it_stops_and_asks() {
    let hops1 = |c: &mut Cfg| c.crew.links = rows(&[("brief", "main", "chat", &["brief"], 0), ("rep", "chat", "main", &["report"], 1)]);
    let (r, _) = flow(&[&report(None)], &[], hops1);
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    let sent: Vec<Value> = events(&r, "crew.sent");
    assert_eq!(sent.iter().filter(|s| s["link"] == "rep" && s["carries"] == "report").count(), 1, "{sent:?}");
    assert_eq!(events(&r, "orchestrate.digest").len(), 1, "the first pass went");
    // the chat has passed the link already: the Log says so, and the report stops
    let (r, _) = flow(&[&report(None)], &[], hops1);
    r.f.rig.log.append(New::of("crew.sent", Actor::Core, json!({ "link": "rep", "carries": "report", "from": "c1", "to": "main" })).thread("c1")).unwrap();
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    let refused = events(&r, "crew.refused");
    assert!(events(&r, "orchestrate.digest").is_empty() && refused.len() == 1, "{}", r.f.rig.dump());
    assert_eq!(refused[0]["why"], "link `rep` has been passed 1 time in this thread: it stops here, ask the operator before another");
    // the count is the thread's own: what another chat passed is not this chat's
    let (r, _) = flow(&[&report(None)], &[], hops1);
    r.f.rig.log.append(New::of("crew.sent", Actor::Core, json!({ "link": "rep", "carries": "report", "from": "c9", "to": "main" })).thread("c9")).unwrap();
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    assert!(events(&r, "crew.refused").is_empty() && events(&r, "orchestrate.digest").len() == 1, "{}", r.f.rig.dump());
}

#[test]
fn an_opinion_a_link_carries_reaches_main_wakes_it_once_and_what_it_followed_is_written_when_its_turn_ends() {
    let (r, claude) = flow(&[&report(Some(("keep the retry loop", "the loop is what the tests hold")))], &["I take it.\nfollowed: o1"], |_| {});
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    assert!(chat_text(&r).contains("the object may also hold \"opinion\""), "the chat is told it may give one: {}", chat_text(&r));
    let op = events(&r, "crew.opinion");
    assert!(op.len() == 1 && op[0]["id"] == "o1" && op[0]["chat"] == "c1" && op[0]["pick"] == "keep the retry loop", "{op:?}");
    let d = events(&r, "orchestrate.digest");
    assert!(d.len() == 1 && d[0]["wake"] == true, "{d:?}");
    // Main is woken for it and is shown the opinion as data, with how to name the one it follows
    r.until("Main was woken", |r| r.f.tasks().iter().any(|t| t.spec.goal.starts_with("J4n: 1 of your chats need a decision")));
    for t in r.f.tasks() {
        r.f.rig.finished(&t.id);
    }
    r.f.front.plan_step("main");
    let shown = claude.starts().into_iter().filter_map(|s| s.text).chain(r.f.tasks().into_iter().map(|t| format!("{} {}", t.spec.goal, t.spec.input))).collect::<Vec<_>>().join("\n");
    assert!(shown.contains("o1 c1: keep the retry loop · the loop is what the tests hold") && shown.contains("`followed: o1`"), "{shown}");
    let decided = events(&r, "crew.decided");
    assert_eq!(decided.len(), 1, "{}", r.f.rig.dump());
    assert_eq!(decided[0]["followed"], "o1");
    assert_eq!(decided[0]["opinions"], json!([{ "id": "o1", "chat": "c1", "pick": "keep the retry loop" }]));
    // said once: another step writes no second one
    r.f.front.plan_step("main");
    assert_eq!(events(&r, "crew.decided").len(), 1);
}

#[test]
fn what_main_followed_is_only_an_opinion_code_gave_out_and_none_is_none() {
    for says in ["It is the wrong pick.\nfollowed: none", "I take it.\nfollowed: o7", "I take it, no line at all"] {
        let (r, _) = flow(&[&report(Some(("keep the retry loop", "the tests hold it")))], &[says], |_| {});
        r.f.front.say("main", LINE);
        until(&r, "the scout ended", |r| r.item("c1").done());
        r.until("Main was woken", |r| r.f.tasks().iter().any(|t| t.spec.goal.starts_with("J4n: 1 of your chats need a decision")));
        for t in r.f.tasks() {
            r.f.rig.finished(&t.id);
        }
        r.f.front.plan_step("main");
        let decided = events(&r, "crew.decided");
        assert!(decided.len() == 1 && decided[0]["followed"].is_null() && decided[0]["opinions"].as_array().unwrap().len() == 1, "{says}: {decided:?}");
    }
}

#[test]
fn an_opinion_no_link_carries_is_left_out_the_report_still_goes_and_the_chat_is_not_told_it_may_give_one() {
    let (r, _) = flow(&[&report(Some(("keep the retry loop", "the tests hold it")))], &[], |c| c.crew.links = rows(&[("brief", "main", "chat", &["brief"], 0), ("rep", "chat", "main", &["report"], 3)]));
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    assert!(!chat_text(&r).contains("opinion"), "{}", chat_text(&r));
    assert!(events(&r, "crew.opinion").is_empty());
    let d = events(&r, "orchestrate.digest");
    assert!(d.len() == 1 && d[0]["wake"] == false, "the report went and Main was not woken: {d:?}");
    let refused = events(&r, "crew.refused");
    assert!(refused.len() == 1 && refused[0]["carries"] == "opinion", "{refused:?}");
    // with the crew off the report's opinion is data nobody reads, and the brief has no word of it
    let (r, _) = flow(&[&report(Some(("keep the retry loop", "the tests hold it")))], &[], |c| c.crew.on = false);
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    assert!(!chat_text(&r).contains("opinion") && events(&r, "crew.opinion").is_empty() && events(&r, "orchestrate.digest")[0]["wake"] == false);
}

// ---- the envoy's places ----

fn envoy_settings(c: &mut Cfg) {
    c.lanes = Switch::Orchestrate;
    c.chats.screen = true;
    c.crew = Crew::shipped();
    c.crew.envoys = true;
    c.doer = Some("alpha".into());
}

fn signed(f: &Fx, chat: &str) {
    let charter = j4n_core::charter::Charter { id: "k1".into(), goal: "tidy".into(), watch: String::new(), watch_every_min: 10, paths: vec!["docs".into()], may: vec!["write".into()], doers: vec![], usd: 1.0, until_ms: None, thread: chat.into(), verbatim: vec![] };
    f.rig.log.append(New::of(j4n_core::charter::SIGNED, Actor::Operator, json!({ "charter": charter, "proposal": "p1" }))).expect("signed");
    f.rig.log.sync();
}

/// An envoy chat with a charter signed in it, and a task of the envoy to call the tool from.
fn envoyed(tune: impl FnOnce(&mut Cfg)) -> (Fx, String, String) {
    let alpha = Scripted::new("alpha", vec![vec![end("envoy")], vec![end("worked")], vec![end("envoy")], vec![end("worked")], vec![end("envoy")], vec![end("worked")], vec![end("envoy")], vec![end("worked")]]);
    let f = fx(vec![alpha], |c| {
        envoy_settings(c);
        tune(c);
    });
    let Said::Enter(chat) = f.front.envoy_said("main") else { panic!("{}", f.rig.dump()) };
    signed(&f, &chat);
    let task = match f.front.say(&chat, "tidy the docs") {
        Said::Started { task } => task,
        other => panic!("{other:?}\n{}", f.rig.dump()),
    };
    f.rig.finished(&task);
    (f, chat, task)
}

fn settle_all(f: &Fx) {
    for t in f.tasks() {
        f.rig.finished(&t.id);
    }
}

#[test]
fn an_envoys_request_goes_by_a_link_and_the_rows_say_how_many_a_charter_may_send() {
    // the star: the request goes, as it did
    let (f, _, envoy) = envoyed(|_| {});
    f.front.envoy_request(&envoy, "k1", "tidy the docs").expect("the star carries it");
    settle_all(&f);
    // no row for it: refused with the reason, and written
    let (f, chat, envoy) = envoyed(|c| c.crew.links = rows(&[("brief", "main", "chat", &["brief"], 0)]));
    let why = f.front.envoy_request(&envoy, "k1", "tidy the docs").unwrap_err();
    assert_eq!(why, format!("no link carries a request from envoy:{chat} to main"));
    assert!(f.rig.of_kind("crew.request").is_empty() && f.rig.of_kind("crew.refused").iter().any(|e| e.data["carries"] == "request" && e.data["why"] == why.as_str()), "{}", f.rig.dump());
    // a row with two hops: the third request is the one refused, as the star's three were
    let (f, _, envoy) = envoyed(|c| c.crew.links = rows(&[("ask", "envoy", "main", &["request"], 2)]));
    f.front.envoy_request(&envoy, "k1", "one").expect("first");
    settle_all(&f);
    f.front.envoy_request(&envoy, "k1", "two").expect("second");
    settle_all(&f);
    let why = f.front.envoy_request(&envoy, "k1", "three").unwrap_err();
    assert_eq!(why, "charter k1 has sent Main 2 requests: ask the operator before another");
    // a row for one envoy chat is not a row for another
    let (f, _, envoy) = envoyed(|c| c.crew.links = rows(&[("ask", "envoy:c99", "main", &["request"], 0)]));
    assert!(f.front.envoy_request(&envoy, "k1", "one").unwrap_err().starts_with("no link carries a request from envoy:"));
}

#[test]
fn a_question_of_the_work_goes_to_its_envoy_only_where_a_link_carries_it_and_is_shown_where_it_was_when_not() {
    let ask = |c: &mut Cfg| c.crew.links = rows(&[("q", "main", "envoy", &["question"], 0)]);
    for (tune, goes) in [(ask as fn(&mut Cfg), true), (|c: &mut Cfg| c.crew.links = rows(&[("b", "main", "chat", &["brief"], 0)]), false), (|_: &mut Cfg| {}, true)] {
        let alpha = Scripted::new("alpha", vec![vec![Beat::Ask(approval("q", "make build")), Beat::Hold, end("done")]]);
        let f = fx(vec![alpha], |c| {
            envoy_settings(c);
            tune(c);
        });
        let Said::Enter(chat) = f.front.envoy_said("main") else { panic!("{}", f.rig.dump()) };
        let mut spec = TaskSpec::new("work for the envoy", "alpha");
        spec.thread = Some("main".into());
        spec.input = json!({ "envoy": { "chat": chat, "charter": "k1" } });
        let task = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
        f.rig.until(&task, "asked", |r| matches!(r.state, State::Waiting { .. }));
        assert_eq!(f.front.envoy_of_task(&task).as_deref(), goes.then_some(chat.as_str()), "{}", f.rig.dump());
        assert_eq!(f.rig.of_kind("crew.refused").len(), usize::from(!goes), "written once: {}", f.rig.dump());
        f.front.envoy_of_task(&task);
        assert_eq!(f.rig.of_kind("crew.refused").len(), usize::from(!goes), "and not at every look");
        // answered where it is shown
        assert!(matches!(f.front.say(if goes { &chat } else { "main" }, "y"), Said::Answered { .. }), "{}", f.rig.dump());
        f.rig.finished(&task);
    }
}

#[test]
fn mains_report_goes_to_the_envoy_for_its_end_check_only_where_a_link_carries_it() {
    let checks = |f: &Fx| f.tasks().iter().filter(|t| t.spec.input.get("endcheck").is_some()).count();
    for (tune, goes) in [((|c: &mut Cfg| c.crew.links = rows(&[("r", "main", "envoy", &["report"], 0)])) as fn(&mut Cfg), true), (|c: &mut Cfg| c.crew.links = rows(&[("q", "main", "envoy", &["question"], 0)]), false)] {
        let (f, chat, _) = envoyed(tune);
        // Main's task for a request of the envoy ended well
        let mut spec = TaskSpec::new("you via c1 · charter k1: tidy", "alpha");
        spec.thread = Some("main".into());
        spec.role = Some("work".into());
        spec.input = json!({ "envoy": { "chat": chat, "charter": "k1" } });
        let main = f.rig.tasks.spawn(spec, Actor::Core).expect("spawns");
        f.rig.finished(&main);
        f.front.plan_step("main");
        assert_eq!(checks(&f), usize::from(goes), "{}", f.rig.dump());
        assert_eq!(f.rig.of_kind("crew.refused").iter().filter(|e| e.data["carries"] == "report").count(), usize::from(!goes), "{}", f.rig.dump());
        settle_all(&f);
    }
}

#[test]
fn with_the_crew_off_no_message_is_held_to_a_row() {
    let (r, _) = flow(&[&report(Some(("keep the retry loop", "the tests hold it")))], &[], |c| {
        c.crew.on = false;
        c.crew.links = rows(&[]);
    });
    r.f.front.say("main", LINE);
    until(&r, "the scout ended", |r| r.item("c1").done());
    assert!(r.f.tasks().iter().any(|t| t.spec.input.get("chat").is_some()) && events(&r, "orchestrate.digest").len() == 1, "{}", r.f.rig.dump());
    for kind in ["crew.sent", "crew.refused", "crew.opinion"] {
        assert!(events(&r, kind).is_empty(), "{kind}");
    }
}
