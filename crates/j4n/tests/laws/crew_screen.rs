//! The crew on the screen, on the real front and the real TUI at 160×44: the seats, the links, the charters, a chat's settings, the opinions and the refusals, each a page of
//! Tasks that shows only while the crew is on; a chat's seat in the list of chats; and the settings page's keys, which write the chat's own layer as the operator's with an
//! undo. Each page is its golden (`J4N_BLESS=1` writes them).

use crate::run_cases::Dirs;
use crate::support::{read_text, root};
use crate::sweep::{Sitting, steady};
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::change::Actor;
use j4n_core::log::New;
use j4n_core::task::Mode;
use j4n_core::value::Value;
use j4n_proto::{Intent, Up};
use serde_json::json;
use std::fs;

fn opts() -> Opts {
    Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Ask), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false }
}

/// The crew on, with its envoys, under the word `orchestrate`, with two links declared.
const ROWS: &str = "api = 1\n[[enable]]\nid = \"crew\"\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n[[setting]]\nid = \"orchestrate.look_model\"\nvalue = \"haiku\"\n[[setting]]\nid = \"lane.model\"\nvalue = \"sonnet\"\n[[setting]]\nid = \"crew.on\"\nvalue = true\n[[setting]]\nid = \"crew.envoys\"\nvalue = true\n[[setting]]\nid = \"crew.links\"\nvalue = \"declared\"\n\
[[link]]\nid = \"report-back\"\nfrom = \"chat\"\nto = \"main\"\ncarries = [\"report\", \"opinion\"]\nhops = 2\n\n[[link]]\nid = \"brief\"\nfrom = \"main\"\nto = \"reviewer\"\ncarries = [\"brief\"]\n";

fn open(tag: &str, rows: &str) -> (Dirs, World) {
    let d = Dirs::new(tag);
    d.committed();
    fs::write(d.j4n_home().join("settings/crew.toml"), rows).unwrap();
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    let world = crate::support::opened(|| wire::open(setup.clone(), &opts(), true, "main"));
    (d, world)
}

/// A chat Main would have started in the seat `reviewer`, and what the crew wrote in the Log.
fn crewed(world: &World) -> Sitting {
    let mut s = Sitting::new(world, (160, 44));
    s.line(":new check the retry loop");
    s.until("the chat is in the list", |t| t.contains("check the retry loop"));
    let log = world.front.log();
    log.append(New::of("chat.seated", Actor::Core, json!({ "seat": "reviewer", "role": "reviewer", "reason": "the brief", "narrowed": [] })).thread("c1")).unwrap();
    log.append(New::of("crew.opinion", Actor::Core, json!({ "id": "o1", "chat": "c1", "pick": "keep the retry loop", "why": "the tests hold it" })).thread("main")).unwrap();
    log.append(New::of("crew.refused", Actor::Core, json!({ "carries": "brief", "from": "main", "to": "c2 (seat scout)", "why": "no link carries a brief from main to c2 (seat scout)" })).thread("main")).unwrap();
    let charter = j4n_core::charter::Charter { id: "k1".into(), goal: "tidy the docs".into(), watch: "docs-check".into(), watch_every_min: 60, paths: vec!["docs".into()], may: vec!["read".into(), "write".into()], doers: vec![], usd: 2.0, until_ms: None, thread: "c9".into(), verbatim: vec![] };
    log.append(New::of(j4n_core::charter::SIGNED, Actor::Operator, json!({ "charter": charter, "proposal": "p1" }))).unwrap();
    log.sync();
    s
}

/// The page is opened as the screen opens one (`Intent::Open`), and its rows are waited for.
fn page(s: &mut Sitting, id: &str, what: &str) -> String {
    s.op.up.send(Up::Intent { intent: Intent::Open { node: id.into() } }).unwrap();
    s.until(what, |t| t.contains(what) && !t.contains("a chat is a thread"))
}

fn golden(name: &str, screen: &str) -> Option<String> {
    let path = root().join(format!("crates/j4n/tests/crew_screen/{name}.160x44.txt"));
    let got = steady(screen);
    if std::env::var_os("J4N_BLESS").is_some_and(|v| !v.is_empty()) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &got).unwrap();
        return None;
    }
    (read_text(&path).as_deref() != Some(got.as_str())).then(|| format!("{}: not its golden; J4N_BLESS=1 writes it\n{got}", path.display()))
}

#[test]
fn the_crew_pages_are_their_goldens_and_a_chats_seat_is_in_the_list() {
    let (_d, world) = open("crew-screen", ROWS);
    assert!(world.notices.is_empty(), "{:?}", world.notices);
    let mut s = crewed(&world);
    let mut found = Vec::new();
    // the list of chats says the seat before the doer
    s.keys(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(1))]);
    let chats = s.until("the seat in the list", |t| t.lines().any(|l| l.contains("c1") && l.contains("reviewer · ")));
    found.extend(golden("chats", &chats));
    for (id, word) in [("seats", "scout"), ("links", "report-back"), ("charters", "tidy the docs"), ("opinions", "keep the retry loop"), ("refusals", "no link carries a brief")] {
        let screen = page(&mut s, id, word);
        found.extend(golden(id, &screen));
    }
    // the settings of the chat the screen is in
    s.op.up.send(Up::Intent { intent: Intent::Enter { chat: "c1".into() } }).unwrap();
    s.settle();
    let screen = page(&mut s, "chatset", "tools.allow");
    found.extend(golden("chatset", &screen));
    world.close();
    crate::support::assert_clean("the crew pages", &found);
}

#[test]
fn a_setting_written_on_the_page_is_the_operators_with_an_undo_and_a_dotted_one_reaches_its_key() {
    let (_d, world) = open("crew-set", ROWS);
    let mut s = crewed(&world);
    for (id, field, value) in [("c1", "effort", json!("high")), ("c1.tools", "deny", json!("Bash"))] {
        s.op.up.send(Up::Intent { intent: Intent::Set { kind: "chat".into(), id: id.into(), field: field.into(), value } }).unwrap();
    }
    s.settle();
    let held = world.front.chat_layer("c1");
    let word = |k: &str| held.iter().find(|(h, _)| h == k).map(|(_, v)| v.to_string());
    let notes: Vec<String> = world.front.log().read(0).filter(|e| e.kind == "chat.note").filter_map(|e| e.data["text"].as_str().map(String::from)).collect();
    assert_eq!(word("effort").as_deref(), Some("\"high\""), "{held:?} {notes:?}");
    assert_eq!(word("tools.deny").as_deref(), Some("[\"Bash\"]"), "{held:?}");
    let rows = world.front.chatset_values("c1");
    let field = |id: &str, k: &str| {
        rows.iter().find_map(|r| match r {
            Value::Map(m) if m.get("id") == Some(&Value::text(id)) => match m.get(k) {
                Some(Value::Str(w)) => Some(w.clone()),
                _ => None,
            },
            _ => None,
        })
    };
    assert_eq!(rows.len(), 5, "every setting a chat may hold has a row, set or not");
    assert_eq!((field("effort", "by").as_deref(), field("effort", "now").as_deref(), field("model", "now").as_deref()), (Some("you"), Some("high"), Some("not set")), "the page says who set it");
    assert_eq!(field("tools.deny", "now").as_deref(), Some("Bash"));
    // the undo of the row, on the page: the key `u` on the effort row takes the last change back (the typed `:undo` is its second way)
    assert!(field("effort", "undo").is_some_and(|u| u.starts_with("cs")), "the row carries the undo");
    s.op.up.send(Up::Intent { intent: Intent::Enter { chat: "c1".into() } }).unwrap();
    s.settle();
    page(&mut s, "chatset", "tools.allow");
    s.keys(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::Down)]);
    s.keys(j4n_tui::headless::typed("u", false));
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while world.front.chat_layer("c1").iter().any(|(k, _)| k == "effort") && std::time::Instant::now() < end {
        s.settle();
    }
    assert!(world.front.chat_layer("c1").iter().all(|(k, _)| k != "effort"), "{}", s.settle());
    assert!(world.front.chat_layer("c1").iter().any(|(k, _)| k == "tools.deny"), "only that change was taken back");
    // a refusal is said, and nothing is written: a key that is machine-wide
    s.op.up.send(Up::Intent { intent: Intent::Set { kind: "chat".into(), id: "c1".into(), field: "route.on".into(), value: json!(true) } }).unwrap();
    s.settle();
    assert!(world.front.chat_layer("c1").iter().all(|(k, _)| k != "route.on"));
    world.close();
}

#[test]
fn with_the_crew_off_there_are_no_crew_pages_and_a_chat_row_has_no_seat() {
    let (_d, world) = open("crew-off", "api = 1\n[[enable]]\nid = \"crew\"\n[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n[[setting]]\nid = \"orchestrate.look_model\"\nvalue = \"haiku\"\n[[setting]]\nid = \"lane.model\"\nvalue = \"sonnet\"\n[[setting]]\nid = \"crew.on\"\nvalue = false\n");
    let mut s = Sitting::new(&world, (160, 44));
    s.line(":new check the retry loop");
    s.until("the chat is in the list", |t| t.contains("check the retry loop"));
    world.front.log().append(New::of("chat.seated", Actor::Core, json!({ "seat": "reviewer", "role": "reviewer", "reason": "the brief", "narrowed": [] })).thread("c1")).unwrap();
    world.front.log().sync();
    let chats = s.keys(vec![j4n_tui::headless::press(j4n_tui::headless::KeyCode::F(1))]);
    assert!(!chats.contains("reviewer"), "{chats}");
    for id in ["seats", "links", "charters", "chatset", "opinions", "refusals"] {
        s.op.up.send(Up::Intent { intent: Intent::Open { node: id.into() } }).unwrap();
        let shown = s.settle();
        assert!(!shown.contains("Seats") && !shown.contains("Refusals"), "{id} is not a page while the crew is off: {shown}");
    }
    world.close();
}
