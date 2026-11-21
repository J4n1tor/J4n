//! A charter from the draft to the signing, with the front, the proposals, the folds and the files the real ones. The card shows the scope in words made by code
//! and asks twice; the second press is the signing, which is the operator's alone and is written to the Log with the operator as the signer; a charter that
//! meets a signed one's path is refused, and a signed one is changed only by the operator; a model may not draft one, and a row put in a file signs nothing.

use crate::run_cases::Dirs;
use crate::screen_drive::Operator;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::charter::Charter;
use j4n_core::task::Mode;
use std::fs;

fn open(d: &Dirs, interactive: bool, approval: Approval) -> World {
    fs::create_dir_all(d.home().join(".j4n/settings")).unwrap();
    fs::write(d.home().join(".j4n/settings/crew.toml"), "api = 1\n[[enable]]\nid = \"crew\"\n[[setting]]\nid = \"crew.on\"\nvalue = true\n").unwrap();
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(approval), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, interactive, "main"))
}

fn charter(id: &str, paths: &[&str]) -> Charter {
    Charter { id: id.into(), goal: "tidy the docs".into(), watch: String::new(), watch_every_min: 10, paths: paths.iter().map(|p| p.to_string()).collect(), may: vec!["read".into(), "write".into()], doers: Vec::new(), usd: 2.0, until_ms: None, thread: "e1".into(), verbatim: Vec::new() }
}

#[test]
fn the_card_shows_the_scope_in_words_asks_twice_and_the_second_press_is_the_operators_signing() {
    let d = Dirs::bare("charter-sign");
    let world = open(&d, true, Approval::Ask);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    assert!(world.front.who().charters_answer(), "on a screen a charter may answer");
    let mut op = Operator::sit(&world);
    let now = world.front.log().clock().now_ms();
    let p = world.front.charter_draft(Actor::Core, &charter("k1", &["docs"])).expect("drafted");
    // nothing answers yet: it is a draft
    assert!(world.front.cfg().charters.list().is_empty());
    op.type_line(":accept");
    let s = op.until("the card's warning", |s| s.contains("widens what J4n may do"));
    let said = s.lines().find(|l| l.contains("widens what J4n may do")).unwrap();
    assert!(said.contains("answers yes, without asking you"), "the scope leads, in words made by code: {s}");
    assert!(s.contains("docs") && s.contains("$2.00") && s.contains("until you end it") || s.contains("spends at most $2.00"), "{s}");
    assert!(world.front.cfg().charters.list().is_empty(), "the first press signs nothing: {s}");
    op.type_line(":accept");
    op.until("the signing", |s| s.contains("charter k1 is signed"));
    let signed = world.front.cfg().charters.live(now);
    assert_eq!(signed.len(), 1);
    assert_eq!(signed[0].proposal, p);
    let ev: Vec<_> = world.front.log().read(0).filter(|e| e.kind == "charter.signed").collect();
    assert_eq!((ev.len(), ev[0].by.clone(), ev[0].data["signer"].as_str()), (1, Actor::Operator, Some("operator")));
    // undoing the accept ends it
    op.type_line(&format!(":undo {p}"));
    op.until("taken back", |s| s.contains("took back"));
    assert!(world.front.cfg().charters.live(now).is_empty(), "an undone signing answers nothing");
    drop(op);
    world.close();
}

#[test]
fn a_model_may_not_draft_one_a_path_may_not_be_shared_a_signed_one_is_changed_by_the_operator_alone_and_a_row_in_a_file_signs_nothing() {
    let d = Dirs::bare("charter-rules");
    let world = open(&d, true, Approval::Ask);
    let mut op = Operator::sit(&world);
    let now = world.front.log().clock().now_ms();
    let why = world.front.charter_draft(Actor::Model("m".into()), &charter("k0", &["docs"])).expect_err("a model may not draft a charter");
    assert!(why.contains("not a kind it may draft"), "{why}");
    let p = world.front.charter_draft(Actor::Core, &charter("k1", &["docs"])).expect("drafted");
    op.type_line(":accept");
    op.until("the card", |s| s.contains("widens what J4n may do"));
    op.type_line(":accept");
    op.until("the signing", |s| s.contains("charter k1 is signed"));
    // a second charter that meets its path is refused at the draft
    let why = world.front.charter_draft(Actor::Core, &charter("k2", &["docs/guide"])).expect_err("shared path");
    assert!(why.contains("two charters may not share a path"), "{why}");
    assert!(world.front.charter_draft(Actor::Core, &charter("k3", &["src"])).is_ok(), "a path nobody holds is free");
    // the one that was signed is changed by the operator alone
    let wider = Charter { paths: vec!["docs".into(), "src".into()], ..charter("k1", &["docs"]) };
    let why = world.front.charter_draft(Actor::Core, &wider).expect_err("a change by anyone but the operator");
    assert!(why.contains("only the operator changes it"), "{why}");
    assert!(world.front.charter_draft(Actor::Operator, &Charter { paths: vec!["docs".into(), "notes".into()], ..charter("k1", &["docs"]) }).is_ok());
    // the operator ends it; no one else does
    assert!(world.front.charter_end(&Actor::Core, "k1").is_err());
    assert!(world.front.cfg().charters.holds("k1", now));
    // a row put in the file is a record and signs nothing
    let row = Charter { paths: vec!["lib".into()], ..charter("k9", &["lib"]) }.draft(now).unwrap();
    world.front.proposer().unwrap().edit(row).expect("the operator's own edit of the row is made");
    assert!(!world.front.cfg().charters.holds("k9", now), "a row is no signing");
    world.front.charter_end(&Actor::Operator, "k1").expect("the operator ends it");
    assert!(!world.front.cfg().charters.holds("k1", now));
    let _ = p;
    drop(op);
    world.close();
}

#[test]
fn a_run_that_denies_or_says_yes_to_all_has_no_charter_to_answer_and_a_piped_run_that_asks_has() {
    for (tag, interactive, approval, answers) in [("deny", false, Approval::Deny, false), ("full", false, Approval::Full, false), ("ask", false, Approval::Ask, true), ("screen", true, Approval::Deny, true)] {
        let d = Dirs::bare(&format!("charter-{tag}"));
        let world = open(&d, interactive, approval);
        assert_eq!(world.front.who().charters_answer(), answers, "{tag}");
        world.close();
    }
}
