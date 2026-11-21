//! J4n proposes changes to itself from what it sees, and nothing is written until the operator says yes — the
//! same yes given three times becomes a proposal to remember it, a machine answerer that is wrong more where it is barely sure
//! becomes a proposal that it be surer — and an accepted proposal can be undone, also after J4n has been started again. The
//! front, the loop, the rules, the folds and the files are the real ones, wired as the binary wires them.

use crate::screen_drive::Operator;
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::gate::grant::here_dir;
use j4n_core::log::New;
use j4n_core::task::Mode;
use serde_json::json;
use std::fs;
use std::time::{Duration, Instant};

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

fn open(d: &Dirs) -> World {
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"))
}

/// What the operator's folder for this project holds in a file, or nothing.
fn held(d: &Dirs, file: &str) -> String {
    fs::read_to_string(here_dir(&d.j4n_home(), &d.project()).join(file)).unwrap_or_default()
}

fn log(world: &World, kind: &str, by: Actor, data: serde_json::Value) {
    world.front.log().append(New::of(kind, by, data)).expect("written");
}

/// Wait until `n` proposals have been made: a rule answers an event a moment after it is written.
fn made(world: &World, n: usize) {
    let end = Instant::now() + crate::support::PATIENCE;
    loop {
        world.front.log().sync();
        if world.front.log().read(0).filter(|e| e.kind == "proposal.made").count() >= n {
            // and the fold has it
            world.front.log().sync();
            return;
        }
        assert!(Instant::now() < end, "no proposal was made");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The operator's yes to one approval.
fn allowed(world: &World, n: u32) {
    let data = json!({ "ask": format!("a{n}"), "via": "human", "answer": "yes", "verb": "shell", "target": "make build", "action": "shell:make build", "action_id": "here-5ca1ab1e", "grantable": true });
    log(world, "ask.answered", Actor::Operator, data);
}

#[test]
fn the_same_yes_three_times_becomes_a_proposal_that_writes_nothing_until_it_is_accepted_twice_and_that_undo_takes_back() {
    let d = Dirs::new("grow-allow");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.until("the tab", |s| s.lines().nth(1).is_some_and(|l| l.contains("Grow")));

    allowed(&world, 1);
    allowed(&world, 2);
    op.type_line(":tips");
    let s = op.until("an empty list", |s| s.contains("nothing has been proposed"));
    assert!(!s.contains("without asking"), "two yeses are not enough: {s}");

    allowed(&world, 3);
    made(&world, 1);
    op.type_line(":tips");
    let s = op.until("the proposal", |s| s.contains("Allow \"make build\" here without asking"));
    assert!(s.contains("open"), "{s}");
    assert_eq!(held(&d, "gate.toml"), "", "a proposal changes nothing");

    // the first press says what it widens; the second writes it
    op.type_line(":accept");
    let s = op.until("what it widens", |s| s.contains("widens what J4n may do"));
    assert!(s.contains(":accept"), "{s}");
    assert_eq!(held(&d, "gate.toml"), "");
    op.type_line(":accept");
    op.until("the yes", |s| s.contains("accepted") && s.contains("1 change(s) written"));
    let file = held(&d, "gate.toml");
    assert!(file.contains("here-5ca1ab1e") && file.contains("verdict = \"allow\"") && file.contains("make build"), "{file}");

    // J4n is started again, and the change can still be taken back
    drop(op);
    drop(world);
    let world = open(&d);
    assert!(world.notices.is_empty(), "the row it wrote is a row the Table reads: {:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":undo");
    op.until("the undo", |s| s.contains("took back 1 change(s)"));
    assert!(!held(&d, "gate.toml").contains("here-5ca1ab1e"), "{}", held(&d, "gate.toml"));
    op.type_line(":tips");
    op.until("it is undone", |s| s.contains("undone"));

    // it does not come back by itself: more of the same yes is not twice the evidence
    allowed(&world, 4);
    std::thread::sleep(Duration::from_millis(300));
    world.front.log().sync();
    assert_eq!(world.front.log().read(0).filter(|e| e.kind == "proposal.made").count(), 1, "nothing new was proposed");
}

#[test]
fn a_proposal_that_is_declined_changes_nothing_and_stays_declined() {
    let d = Dirs::new("grow-decline");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    (1..=3).for_each(|n| allowed(&world, n));
    made(&world, 1);
    op.type_line(":tips");
    op.until("the proposal", |s| s.contains("without asking"));
    op.type_line(":decline");
    op.until("the no", |s| s.contains("declined p"));
    assert_eq!(held(&d, "gate.toml"), "");
    op.type_line(":accept");
    let s = op.until("the refusal", |s| s.contains("no open proposal"));
    assert_eq!(held(&d, "gate.toml"), "", "{s}");
    // twice the evidence, though, is a new proposal
    (4..=6).for_each(|n| allowed(&world, n));
    made(&world, 2);
}

#[test]
fn the_switch_in_the_operators_own_folder_stops_the_tips() {
    let d = Dirs::new("grow-off");
    let home = here_dir(&d.j4n_home(), &d.project());
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join("settings.toml"), "api = 1\n[[setting]]\nid = \"grow.tips\"\nvalue = false\n").unwrap();
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    (1..=6).for_each(|n| allowed(&world, n));
    std::thread::sleep(Duration::from_millis(400));
    world.front.log().sync();
    assert_eq!(world.front.log().read(0).filter(|e| e.kind == "proposal.made").count(), 0, "with the switch off nothing is proposed");
}

/// An answer of a decider, and what happened to it.
fn answer(world: &World, n: u32, conf: f64, wrong: bool) {
    let ask = format!("m{n}");
    log(world, "ask.answered", Actor::Model("decider:jev".into()), json!({ "ask": ask, "point": "intake", "by": "decider:jev", "kind": "decider", "conf": conf, "answer": "task", "via": "chain" }));
    log(world, "label", Actor::Core, json!({ "subject_kind": "answer", "subject": ask, "producer": "decider:jev", "point": "intake", "label": if wrong { "overridden" } else { "held" }, "weight": if wrong { 1.0 } else { 0.0 } }));
}

#[test]
fn a_min_conf_proposal_appears_when_the_barely_sure_answers_are_wrong_more_and_undo_restores_the_old_value() {
    let d = Dirs::new("grow-minconf");
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);

    // twelve answers well above the line, none wrong; then forty-four just over it, eight wrong: 18 % against 0 %
    (1..=12).for_each(|n| answer(&world, n, 0.92, false));
    (13..=56).for_each(|n| answer(&world, n, 0.64, n < 21));
    made(&world, 1);
    op.type_line(":tips");
    let s = op.until("the proposal", |s| s.contains("Ask decider:jev to be surer at intake"));
    assert!(s.contains("open"), "{s}");
    assert_eq!(held(&d, "ask_point.toml"), "", "nothing changes until the operator says so");

    op.type_line(":accept");
    op.until("the yes", |s| s.contains("1 change(s) written"));
    let file = held(&d, "ask_point.toml");
    assert!(file.contains("decider:jev?min_conf=0.65"), "0.6 and a step of 0.05: {file}");

    // the point asks for more now, when J4n is started again; and undo puts the old value back
    drop(op);
    drop(world);
    let world = open(&d);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut op = Operator::sit(&world);
    op.type_line(":undo");
    op.until("the undo", |s| s.contains("took back 1 change(s)"));
    assert!(!held(&d, "ask_point.toml").contains("min_conf=0.65"), "{}", held(&d, "ask_point.toml"));
}

#[test]
fn a_draft_a_model_wrote_is_held_to_the_author_gate() {
    let d = Dirs::new("grow-author");
    let world = open(&d);
    let proposer = world.front.proposer().expect("a proposer").clone();
    let step = |kind: &str, id: &str| j4n_core::proposal::Step {
        to: j4n_core::change::Where::User,
        file: None,
        kind: kind.into(),
        id: id.into(),
        op: j4n_core::proposal::StepOp::Set("doc = \"a thing\"".parse().unwrap()),
    };
    let draft = |kind: &str| j4n_core::proposal::Draft { key: format!("k-{kind}"), title: "t".into(), why: "w".into(), evidence: 1, steps: vec![step(kind, "x")], widens: Vec::new() };
    for kind in ["gate", "connector", "model", "enable"] {
        let e = proposer.make(draft(kind), Actor::Model("m".into()), None).unwrap_err().msg;
        assert!(e.contains("not a kind it may draft"), "{kind}: {e}");
    }
}

const NOISY: &str = "api = 1\nid = \"noisy\"\nversion = \"0.1.0\"\nsummary = \"says things\"\nneeds = [\"base\"]\nstability = \"experimental\"\n";
const NOISY_RULES: &str = "api = 1\n[[rule]]\nid = \"noisy-say\"\non = \"task.settled\"\nthen = \"notice\"\ntext = \"done\"\n";

#[test]
fn a_pack_is_turned_on_and_off_by_a_proposal_and_one_that_adds_a_rule_asks_twice() {
    let d = Dirs::new("pack-propose");
    crate::screen_packs::install(&d);
    let packs = j4n_core::pack::packs_dir(&d.j4n_home());
    fs::create_dir_all(packs.join("noisy/rules")).unwrap();
    fs::write(packs.join("noisy/pack.toml"), NOISY).unwrap();
    fs::write(packs.join("noisy/rules/r.toml"), NOISY_RULES).unwrap();
    let harness = here_dir(&d.j4n_home(), &d.project()).join("harness.toml");
    {
        let world = open(&d);
        assert!(world.notices.is_empty(), "{:?}", world.notices);
        let mut op = Operator::sit(&world);
        // a pack that adds only a view, a page, a kind and a command is turned on at once, and the row pins its files
        op.type_line(":pack on widgets");
        op.until("it is on", |s| s.contains("widgets is on from the next start"));
        let row = fs::read_to_string(&harness).unwrap();
        assert!(row.contains("id = \"widgets\"") && row.contains(&format!("hash = \"{}\"", crate::screen_packs::pin(&d))), "{row}");
        // one that adds a rule is proposed, and says what it widens at the first accept
        op.type_line(":pack on noisy");
        op.until("a proposal", |s| s.contains("proposed p") && s.contains("adds 1 rule"));
        assert!(!fs::read_to_string(&harness).unwrap().contains("noisy"));
        op.type_line(":accept");
        op.until("the warning", |s| s.contains("widens what J4n may do"));
        assert!(!fs::read_to_string(&harness).unwrap().contains("noisy"));
        op.type_line(":accept");
        op.until("the second yes", |s| s.contains("1 change(s) written"));
        assert!(fs::read_to_string(&harness).unwrap().contains("noisy"));
        op.type_line(":pack on noisy");
        op.until("already", |s| s.contains("noisy is on already") || s.contains("proposed already"));
    }
    // started again, both are on; the page of the first is there
    {
        let world = open(&d);
        assert!(world.notices.is_empty(), "{:?}", world.notices);
        let mut op = Operator::sit(&world);
        op.until("the page of the pack", |s| s.lines().nth(1).is_some_and(|l| l.contains("Shelf")));
        // turning one off widens nothing, whatever it adds: one command
        op.type_line(":pack off noisy");
        op.until("it is off", |s| s.contains("noisy is off from the next start"));
        assert!(fs::read_to_string(&harness).unwrap().contains("on = false"));
        op.type_line(":pack off widgets");
        op.until("it is off", |s| s.contains("widgets is off from the next start"));
        op.type_line(":undo");
        op.until("taken back", |s| s.contains("took back 1 change(s)"));
        assert!(fs::read_to_string(&harness).unwrap().contains("widgets"), "the undo of the off is the row it was");
    }
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.until("the page again", |s| s.lines().nth(1).is_some_and(|l| l.contains("Shelf")));
    op.type_line(":pack on nothing");
    op.until("no such pack", |s| s.contains("there is no pack nothing"));
}

/// Wait for a file of the operator's folder to hold (or stop holding) a word.
fn file_has(d: &Dirs, file: &str, word: &str, yes: bool) {
    let end = Instant::now() + crate::support::PATIENCE;
    while held(d, file).contains(word) != yes {
        assert!(Instant::now() < end, "{file} {} {word}: {}", if yes { "never held" } else { "still holds" }, held(d, file));
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_proposals_page_accepts_through_the_card_and_takes_back_with_a_key() {
    use j4n_tui::headless::{KeyCode, press};
    // the page has its column titles, so the columns are as wide as the titles: the state of p5 is the second word of its row
    let state = |s: &str, is: &str| s.lines().any(|l| l.split_whitespace().take(3).collect::<Vec<_>>().windows(2).any(|w| w == ["p5", is]));
    let d = Dirs::new("grow-page");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    (1..=3).for_each(|n| allowed(&world, n));
    made(&world, 1);
    op.type_line(":proposals");
    let s = op.until("the page", |s| s.contains("Allow \"make build\" here without asking"));
    assert!(state(&s, "open") && !state(&s, "accepted"), "{s}");
    // enter does the first thing the row offers: accept; it is risky, so a card asks first, and nothing is written until `y`
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the card", |s| s.contains("accept p5?") && s.contains("y yes · n no"));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(held(&d, "gate.toml"), "", "the card is only asking");
    // this one widens: the core arms it on a first `:accept` and applies it on a second, and the command of the key (`:accept p5 yes`) is both
    op.press(vec![press(KeyCode::Char('y'))]);
    file_has(&d, "gate.toml", "here-5ca1ab1e", true);
    let s = op.until("accepted", |s| state(s, "accepted"));
    assert!(!state(&s, "open"), "{s}");
    // the key of the row takes it back
    op.press(vec![press(KeyCode::Char('u'))]);
    file_has(&d, "gate.toml", "here-5ca1ab1e", false);
    op.until("undone", |s| s.contains("undone"));
}
