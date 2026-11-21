//! A refusal leads somewhere. The note that says why carries the act that mends it, and `⏎` on
//! an empty composer does it while the note is the chat's newest row. `:chats` while the chats are off offers the Features switch, so
//! the first refusal a new user meets leads on; a command of a pack that is off turns the pack on; the first start's row, a line no agent can take and the
//! Setup card each open Connect › Setup, and the card asks once per J4n home.
//! The real front, Serve, proposer and TUI, with keys only.

use crate::screen_drive::{Operator, flat};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Intent, Row, Up};
use j4n_tui::headless::{KeyCode, press};
use serde_json::{Value as Json, json};

fn open(d: &Dirs) -> World {
    open_in(d, &d.project(), Some("test"))
}

/// The world of the project `root` in the home of `d`, with the launch's pin `doer` (none: a line's doer is elected), and no `claude` at all.
fn open_in(d: &Dirs, root: &std::path::Path, doer: Option<&str>) -> World {
    let opts = Opts { doer: doer.map(String::from), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: root.to_path_buf(), home: d.j4n_home(), claude: d.dir.join("bin/no-claude").display().to_string() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn events(w: &World, kind: &str) -> Vec<Json> {
    w.front.log().sync();
    w.front.log().read(0).filter(|e| e.kind == kind).map(|e| e.data).collect()
}

/// The act the newest note carries, as the Log holds it.
fn last_act(w: &World) -> Option<Json> {
    events(w, "chat.note").last().and_then(|n| n.get("act").cloned())
}

fn row(serve: &mut Serve, view: &str, key: &str) -> Row {
    let (downs, _) = serve.up(Up::Watch { bind: format!("view:{view}") });
    let rows = downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten();
    rows.into_iter().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key} in {view}"))
}

fn wait_for(op: &mut Operator, what: &str, done: impl Fn() -> bool) {
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !done() {
        assert!(std::time::Instant::now() < end, "never saw {what}; the screen is\n{}", op.look());
        op.until("a moment", |_| true);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn chats_while_off_lead_to_the_features_switch_and_enter_turns_them_on() {
    let d = Dirs::new("refusal-chats");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    // F1: the refusal names the switch and the key, never a file or a `:set`
    op.type_line(":chats");
    op.until("the refusal that leads", |s| flat(s).contains("the chats are off") && flat(s).contains("⏎ turns them on") && flat(s).contains("Settings › All › chats.on"));
    let act = last_act(&world).expect("the refusal carries its act");
    assert_eq!(act["intent"], json!({ "intent": "set", "kind": "setting", "id": "chats.on", "field": "value", "value": true }), "{act}");
    // ⏎ on the empty composer: the operator's edit, applied at once, with no Grow row
    op.press(vec![press(KeyCode::Enter)]);
    wait_for(&mut op, "the switch set", || events(&world, "edit.applied").iter().any(|e| e["title"] == "chats.on: true"));
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    // the Features row says it waits for the next start, once the settings are read again
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    loop {
        let chats = row(&mut serve, "settings", "chats.on");
        let edit = chats.data.as_ref().unwrap()["edit"].clone();
        if edit["waits"].as_bool() == Some(true) {
            break;
        }
        assert!(std::time::Instant::now() < end, "the Features row never said it waits: {edit}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "nothing in Grow");
    // chats.on is wired at the start: its note says so, and its own ⏎ is the restart, which is not pressed here
    op.until("the next start said", |s| flat(s).contains("chats.on is true") && flat(s).contains("⏎ restarts now"));
    assert_eq!(last_act(&world).map(|a| a["intent"].clone()), Some(json!({ "intent": "run", "command": "restart", "args": [] })));
    // the act is the newest row's only: a line said after it takes the bare ⏎ away from it
    op.type_line(":tips");
    // the answer of `:tips` itself: the screen's last line is not empty with the note alone, and a ⏎ pressed then reaches the note
    op.until("the answer of :tips after the note", |s| flat(s).contains("nothing has been proposed"));
    let before = events(&world, "edit.applied").len();
    op.press(vec![press(KeyCode::Enter)]);
    assert!(!world.front.restart_asked(), "an older note's ⏎ is not done");
    assert_eq!(events(&world, "edit.applied").len(), before);
    world.close();
}

#[test]
fn a_command_of_a_pack_that_is_off_leads_to_turning_it_on() {
    let d = Dirs::new("refusal-pack");
    d.pack_off("inbox");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(":inbox");
    op.until("the refusal", |s| flat(s).contains("the pack inbox is not on") && flat(s).contains("⏎ turns it on"));
    op.press(vec![press(KeyCode::Enter)]);
    op.until("the pack on from the next start", |s| flat(s).contains("inbox is on from the next start") && flat(s).contains("⏎ restarts now"));
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "the operator's edit, no Grow row");
    // Grow › Packs has the same key on the row
    let packs = world.front.screen().pack_rows();
    let qc = packs.iter().find(|r| r.key == "qc").expect("the pack qc");
    let keys: Vec<(&str, &Intent, u8)> = qc.actions.iter().map(|a| (a.key.as_str(), &a.intent, a.risk)).collect();
    // qc widens what J4n may do: the key asks on the card first, and the card's yes is the whole of it
    let on = Intent::Run { command: "pack".into(), args: vec!["on".into(), "qc".into(), "yes".into()] };
    assert_eq!(keys, [("enter", &on, 60), (" ", &on, 60)]);
    assert!(packs.iter().find(|r| r.key == "base").unwrap().actions.is_empty(), "base is never off");
    world.close();
}

/// Connect › Setup on the screen: its steps.
fn on_setup(s: &str) -> bool {
    s.contains("Agent") && s.contains("Decider") && s.contains("Custom")
}

#[test]
fn the_first_start_row_opens_connect_setup() {
    // the pack accounts is on here, and its Add account is still not where the row leads: Setup's rows mend each part
    let d = Dirs::new("refusal-first");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    // what the first start in a new folder says when a doer is missing, with the key that mends it
    world.front.first_note("main", "here · pi not found · ⏎ opens Connect › Setup");
    op.until("the first-start row", |s| flat(s).contains("pi not found · ⏎ opens Connect › Setup"));
    assert_eq!(last_act(&world).map(|a| a["intent"].clone()), Some(json!({ "intent": "open", "node": "setup" })));
    op.press(vec![press(KeyCode::Enter)]);
    op.until("Connect › Setup", on_setup);
    world.close();
}

/// No agent is installed and signed in, and none is pinned: the first line is refused with words that say so and the key that opens
/// Connect › Setup.
#[test]
fn a_line_no_agent_can_take_is_refused_with_the_key_that_opens_connect_setup() {
    let d = Dirs::bare("refusal-noagent");
    let world = open_in(&d, &d.project(), None);
    let mut op = Operator::sit(&world);
    op.type_line("make the tests pass");
    op.until("the refusal", |s| flat(s).contains("no agent works") && flat(s).contains("⏎ opens Connect › Setup"));
    assert_eq!(last_act(&world).map(|a| a["intent"].clone()), Some(json!({ "intent": "open", "node": "setup" })));
    assert!(events(&world, "task.queued").is_empty(), "no task was started");
    op.press(vec![press(KeyCode::Enter)]);
    op.until("Connect › Setup", |s| on_setup(s) && s.contains("none works"));
    world.close();
}

/// In J4n basic the pack `jev` is off, and Connect › Deciders is not an empty list with no word: it is one row that says
/// why and whose `⏎` turns the pack on, the same command as Grow › Packs.
#[test]
fn connect_deciders_with_the_pack_jev_off_has_one_row_whose_enter_turns_it_on() {
    let d = Dirs::new("refusal-deciders");
    d.pack_off("jev");
    let world = open(&d);
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: "view:connector-deciders".into() });
    let rows: Vec<Row> = downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect();
    assert_eq!(rows.len(), 1, "one row, not an empty list: {rows:?}");
    let words = rows[0].cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
    assert!(words.contains("jev") && words.contains("off") && words.contains("fixed pick") && words.contains("⏎ turns the pack on"), "{words}");
    let enter: Vec<&Intent> = rows[0].actions.iter().filter(|a| a.key == "enter").map(|a| &a.intent).collect();
    assert_eq!(enter, [&Intent::Say { text: ":pack on jev".into(), with: Vec::new() }], "{:?}", rows[0].actions);
    world.close();
}

fn asks(serve: &mut Serve) -> Vec<j4n_proto::AskView> {
    let (downs, _) = serve.up(Up::Watch { bind: "asks:open".into() });
    downs.into_iter().filter_map(|d| if let Down::Asks { open } = d { Some(open) } else { None }).flatten().collect()
}

/// The Setup card the core holds for the screen now, if any.
fn card(world: &World) -> Option<j4n_proto::AskView> {
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    asks(&mut serve).into_iter().find(|a| a.ask == "setup:card")
}

/// The home's own layer says the card was answered.
fn answered(d: &Dirs) -> bool {
    std::fs::read_to_string(d.j4n_home().join("setup.toml")).is_ok_and(|t| t.contains("setup.card") && t.contains("value = false"))
}

/// A fresh J4n home (J4n basic: the Decider is off): the start raises the Setup card, which says where the agent, the Decider and the test
/// stand. `esc` puts it off and marks nothing, so the next start asks again; `⏎` opens Connect › Setup and answers it in the home's own
/// layer, so neither this project nor a second one of the same home asks again. The old welcome card is folded into it: the Decider is
/// set up on Setup's own row.
#[test]
fn a_fresh_home_raises_the_setup_card_once_esc_puts_it_off_and_its_answer_holds_for_every_project() {
    let d = Dirs::bare("refusal-card");
    let world = open(&d);
    world.front.setup_card_open();
    let a = card(&world).expect("a fresh home raises the card");
    assert_eq!((a.shape, a.asker.as_str(), a.question.as_str()), (j4n_proto::Shape::Approval, "j4n", "Set up J4n?"));
    assert_eq!(a.options.iter().map(|o| (o.id.as_str(), o.label.as_str(), o.recommended)).collect::<Vec<_>>(), [("yes", "open Setup", true), ("no", "later", false)]);
    assert!(a.effect.contains("Agent: test · ready · pinned at start") && a.effect.contains("Decider: not set") && a.effect.contains("Test: not run"), "{}", a.effect);
    assert!(a.why.contains("Connect › Setup") && a.why.contains("asks again at the next start"), "{}", a.why);
    assert_eq!(events(&world, "setup.asked").len(), 1);
    // asked once in a run: a second look raises no second card
    world.front.setup_card_open();
    assert_eq!(events(&world, "setup.asked").len(), 1);
    // esc on the card is its later: it goes, and nothing is marked
    let mut op = Operator::sit(&world);
    op.until("the card", |s| s.contains("Set up J4n?"));
    op.press(vec![press(KeyCode::Esc)]);
    wait_for(&mut op, "the card put off", || events(&world, "setup.answered").iter().any(|e| e["answer"] == "later"));
    op.until("the card gone", |s| !s.contains("Set up J4n?"));
    assert!(card(&world).is_none() && !answered(&d), "later marks nothing");
    drop(op);
    world.close();
    drop(world);
    // the next start asks again; its first option, open Setup, opens the page and answers it for the home
    let world = open(&d);
    world.front.setup_card_open();
    assert!(card(&world).is_some(), "put off, so asked again");
    let mut op = Operator::sit(&world);
    op.until("the card", |s| s.contains("Set up J4n?"));
    op.press(vec![press(KeyCode::Char('1'))]);
    op.until("Connect › Setup", on_setup);
    wait_for(&mut op, "the home marked", || answered(&d));
    // the project's Log keeps what was said at each start, a record only: the mark is the home's
    assert_eq!(events(&world, "setup.answered").iter().map(|e| e["answer"].clone()).collect::<Vec<_>>(), [json!("later"), json!("open")]);
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "the answer is the operator's own edit, no Grow row");
    world.front.setup_card_open();
    assert!(card(&world).is_none() && events(&world, "setup.asked").len() == 2, "answered: not asked again");
    drop(op);
    world.close();
    drop(world);
    // a second project of the same home is not asked
    let other = d.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    let world = open_in(&d, &other, Some("test"));
    world.front.setup_card_open();
    assert!(card(&world).is_none() && events(&world, "setup.asked").is_empty(), "answered once for the home");
    world.close();
}

/// An agent that works and the Decider on: nothing is left to set up, and no card is raised.
#[test]
fn the_setup_card_is_not_raised_when_an_agent_works_and_the_decider_is_on() {
    let d = Dirs::new("refusal-nocard");
    d.defaults_as_shipped();
    let world = open(&d);
    assert!(world.front.screen().packs.iter().any(|p| p.id == "jev" && p.on), "the Decider's pack is on here");
    world.front.setup_card_open();
    assert!(card(&world).is_none() && events(&world, "setup.asked").is_empty());
    world.close();
}
