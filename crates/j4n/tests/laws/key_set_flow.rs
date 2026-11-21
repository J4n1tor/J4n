//! A provider's key is set with keys only. Agents › Providers, `⏎` on the own
//! agent's row opens the secret field, the key is typed masked and `⏎` sends it once; the row says `set · J4n file`, Grow has no row, the
//! Log has `key.set` and no part of the key, and the doer reads it at its next start with no restart. The real wire, front, Serve and TUI.

use crate::screen_drive::{Operator, flat};
use crate::run_cases::Dirs;
use j4n::cli::{Approval, Opts};
use j4n::wire::{self, Setup, World};
use j4n_core::front::serve::Serve;
use j4n_core::task::Mode;
use j4n_proto::{Down, Row, Up};
use j4n_tui::headless::{KeyCode, press, typed};
use std::os::unix::fs::PermissionsExt;

const KEY: &str = "sk-or-keyset-flow-5d0e8b1c77";

fn open(d: &Dirs) -> World {
    let opts = Opts { doer: Some("test".into()), model: None, approval: Some(Approval::Deny), wall: None, usd: None, mode: Mode::Write, place: None, shape: false, anywhere: false };
    let setup = Setup { root: d.project(), home: d.j4n_home(), claude: "claude".into() };
    crate::support::opened(|| wire::open(setup.clone(), &opts, true, "main"))
}

fn providers(world: &World) -> Vec<Row> {
    let mut serve = Serve::new(world.front.clone());
    serve.up(Up::Hello { api: j4n_proto::API });
    let (downs, _) = serve.up(Up::Watch { bind: "view:providers".into() });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().collect()
}

fn log_text(world: &World) -> String {
    world.front.log().sync();
    world.front.log().read(0).map(|e| format!("{} {}", e.kind, e.data)).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_key_set_with_keys_only_says_set_has_no_grow_row_and_the_log_has_no_value() {
    let d = Dirs::new("key-set-flow");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    let rows = providers(&world);
    // the row is the provider's, OpenRouter, whose key is typed once for every connector that names it (`light`, `own`, `span`)
    let at = rows.iter().position(|r| r.key == "openrouter").expect("OpenRouter is a provider");
    assert!(rows[at].cells.iter().any(|c| c.text == "not set"), "{:?}", rows[at]);
    // Agents › Providers, the cursor to `own`, ⏎ opens its secret field
    op.type_line(":providers");
    op.until("Agents › Providers", |s| s.contains("Providers") && s.contains("not set"));
    op.press(vec![press(KeyCode::Home)]);
    op.press((0..at).map(|_| press(KeyCode::Down)).collect());
    op.press(vec![press(KeyCode::Enter)]);
    // typed masked: the screen never shows the key while it is typed
    op.press(typed(KEY, false));
    assert!(!op.look().contains(&KEY[..10]), "the field masks what is typed");
    op.press(vec![press(KeyCode::Enter)]);
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !log_text(&world).contains("key.set") {
        assert!(std::time::Instant::now() < end, "never saw key.set; the screen is\n{}", op.look());
        op.look();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    op.until("the row says it is set", |s| flat(s).contains("set · J4n file"));
    // no Grow row (not a Proposal), no Change, and the Log holds no part of the key
    assert!(world.front.proposer().unwrap().proposals().rows().is_empty(), "nothing in Grow");
    let log = log_text(&world);
    assert!(log.contains(r#"key.set {"connector":"light"}"#), "{log}");
    assert!(!log.contains(&KEY[..10]), "the Log holds no part of the key:\n{log}");
    assert!(!log.contains("edit.applied"), "a key is not an edit with an undo:\n{log}");
    assert!(!op.look().contains(&KEY[..10]));
    // J4n's own key file, mode 0600
    let file = d.j4n_home().join("keys/own");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), KEY, "the key is kept for each connector that names the same reference");
    assert_eq!(std::fs::read_to_string(d.j4n_home().join("keys/light")).unwrap(), KEY);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    // live: the own agent's probe reads it now, with no restart
    let own = world.front.who().doers().iter().find(|d| d.id() == "own").cloned().expect("the own agent");
    assert!(own.probe().authed, "the doer reads the key at its next start");
    drop(op);
    world.close();
}

/// A made-up key of no vendor's shape, pasted on a chat line by mistake.
const LONE: &str = "Hq3cs3LoneLine8b1f4a7d2e9c0b6a5f3e1d7c4b";

/// A line that is one key-like word is held on a card. `drop it` sends nothing: the Log has no `chat.say` and no piece of it. `keep it as
/// a key`, with the provider picked on the card, writes `key.set` and nothing else, and J4n's key file has it. The real wire, front, Serve and TUI.
#[test]
fn a_lone_key_on_a_line_is_held_drop_sends_nothing_and_keep_writes_key_set_only() {
    let d = Dirs::new("key-set-lone-key");
    let world = open(&d);
    let mut op = Operator::sit(&world);
    op.type_line(LONE);
    let s = op.until("the card", |s| s.contains("looks like a key"));
    assert!(!s.contains(&LONE[..8]), "the card shows nothing of it:\n{s}");
    op.press(vec![press(KeyCode::Char('3'))]);
    op.until("the card gone", |s| !s.contains("looks like a key"));
    crate::screen_drive::wait_idle(&world);
    let log = log_text(&world);
    assert!(!log.contains("chat.say") && !log.contains(&LONE[..8]), "drop sends nothing:\n{log}");
    // again, and keep it as a key: the card asks which provider, from the rows of Agents › Providers
    op.type_line(LONE);
    op.until("the card", |s| s.contains("looks like a key"));
    op.press(vec![press(KeyCode::Char('1'))]);
    op.until("the providers", |s| s.contains("which provider") && s.contains("openrouter"));
    let keyed: Vec<Row> = providers(&world).into_iter().filter(|r| r.actions.iter().any(|a| a.key == "enter" && matches!(&a.intent, j4n_proto::Intent::Set { kind, .. } if kind == "key"))).collect();
    let card = op.look();
    assert!(keyed.len() <= 5, "the card shows five options: {} rows take a key", keyed.len());
    for r in &keyed {
        assert!(card.contains(&r.cells[0].text), "`{}` is on the card:\n{card}", r.cells[0].text);
    }
    let n = keyed.iter().position(|r| r.key == "openrouter").expect("OpenRouter takes a key") + 1;
    let id = match &keyed[n - 1].actions.iter().find(|a| a.key == "enter").expect("its key").intent {
        j4n_proto::Intent::Set { id, .. } => id.clone(),
        _ => unreachable!(),
    };
    op.press(vec![press(KeyCode::Char(char::from_digit(n as u32, 10).expect("one of the first nine")))]);
    let end = std::time::Instant::now() + crate::support::PATIENCE;
    while !log_text(&world).contains("key.set") {
        assert!(std::time::Instant::now() < end, "never saw key.set; the screen is\n{}", op.look());
        op.look();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    crate::screen_drive::wait_idle(&world);
    let log = log_text(&world);
    assert!(log.contains(&format!(r#"key.set {{"connector":"{id}"}}"#)), "{log}");
    assert!(!log.contains("chat.say") && !log.contains(&LONE[..8]), "keep writes key.set only, and no piece of the key:\n{log}");
    assert!(!op.look().contains(&LONE[..8]));
    assert_eq!(std::fs::read_to_string(d.j4n_home().join("keys").join(&id)).unwrap(), LONE, "kept in J4n's own key file");
    drop(op);
    world.close();
}
