//! A provider's key typed in Agents › Providers is kept in J4n's own key file, mode 0600,
//! written by a rename; the Log gets `key.set` with the connector and no value; the redactor scrubs the value from the moment it is typed and
//! from the file at every start; `env:` wins when it is set; `:key remove` (the row's `x`) writes `key.removed`.

use crate::front_cases::{Fx, fx_screened};
use crate::world::Scratch;
use j4n_core::front::screen::Screen;
use j4n_core::front::serve::Serve;
use j4n_core::keys::{self, Keys};
use j4n_core::redact::Redactor;
use j4n_core::setting::Settings;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_proto::{Down, Intent, Row, Secret, Up};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;

const VALUE: &str = "sk-ts7b-the-value-9f3c2a71";

fn mode(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_key_is_one_file_of_mode_0600_written_by_a_rename_and_registered_at_every_start() {
    let home = Scratch::new("ts7b-keys");
    let keys = Keys::new(home.path());
    let red = Redactor::new();
    keys.set(&red, "own", &format!("  {VALUE}\n")).unwrap();
    let file = home.join("keys/own");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), VALUE, "kept trimmed, nothing else in the file");
    assert_eq!((mode(&file), mode(&home.join("keys"))), (0o600, 0o700));
    let left: Vec<_> = std::fs::read_dir(home.join("keys")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, ["own"], "no file beside it is left from the rename");
    assert_eq!(red.scrub(&format!("key={VALUE}")), "key=«secret:OWN_KEY»", "the redactor knows it from the moment it is typed");
    // typed again: replaced; a value too short to keep out of the Log, a second line, or nothing is refused and the old key stays
    keys.set(&red, "own", "sk-ts7b-another-value-4411").unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "sk-ts7b-another-value-4411");
    for bad in ["abc", "two\nlines", "   "] {
        assert!(keys.set(&red, "own", bad).is_err(), "{bad:?} is refused");
    }
    assert!(keys.set(&red, "../own", VALUE).is_err() && keys.path("../own").is_none(), "an id that is no file's name names no file");
    // the next start: a new redactor is told of every key from the files
    let fresh = Redactor::new();
    assert!(keys.register_all(&fresh).is_empty());
    assert_eq!(fresh.scrub("sk-ts7b-another-value-4411"), "«secret:OWN_KEY»");
    assert!(keys.remove("own").unwrap() && !file.exists());
    assert!(!keys.remove("own").unwrap(), "removing it twice says there was none");
}

#[test]
fn env_wins_over_the_key_j4n_keeps_and_the_row_says_where_it_comes_from() {
    let home = Scratch::new("ts7b-env");
    let keys = Keys::new(home.path());
    // a name only this test uses: set while the test runs and nowhere else
    let name = "J4N_TS7B_ENV_WINS_KEY";
    let reference = format!("env:{name}");
    assert_eq!(keys::state(Some(&keys), "gw", &reference), "not set");
    keys.set(&Redactor::new(), "gw", VALUE).unwrap();
    assert_eq!(keys::state(Some(&keys), "gw", &reference), "set · J4n file");
    // SAFETY: the name is this test's own; no other test reads or writes it
    unsafe { std::env::set_var(name, "sk-from-the-environment-0001") };
    assert_eq!(keys::state(Some(&keys), "gw", &reference), "set · env", "env wins when it is set");
    unsafe { std::env::remove_var(name) };
    assert_eq!(keys::state(Some(&keys), "gw", &reference), "set · J4n file");
    // a row's own `file:` counts after J4n's
    let own = home.join("its-key");
    std::fs::write(&own, "sk-its-own-file-0002").unwrap();
    std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(keys::state(None, "jev", &format!("file:{}", own.display())), "set · file");
    assert_eq!(keys::state(None, "jev", "file:/nowhere/at/all"), "not set");
}

/// A key that is there and that its connector refuses (a file others can read, a value too short to keep out of the Log) is not
/// called set: its row says why, and a place after it that gives a key still wins.
#[test]
fn a_key_that_is_there_and_refused_says_not_used_and_why_on_its_row() {
    let home = Scratch::new("key-refused");
    let keys = Keys::new(home.path());
    let file = |name: &str, text: &str, mode: u32| {
        let p = home.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        format!("file:{}", p.display())
    };
    assert_eq!(keys::state(None, "jev", &file("wide", "sk-its-own-file-0003", 0o644)), "not used · others can read the file");
    assert_eq!(keys::state(None, "jev", &file("short", "ab12z", 0o600)), "not used · too short");
    // J4n's own file, widened by hand, is refused as well; a good row file after it is what is used
    keys.set(&Redactor::new(), "jev", VALUE).unwrap();
    std::fs::set_permissions(home.join("keys/jev"), std::fs::Permissions::from_mode(0o640)).unwrap();
    assert_eq!(keys::state(Some(&keys), "jev", "env:J4N_KEY_REFUSED_NOBODY_SETS"), "not used · others can read the file");
    assert_eq!(keys::state(Some(&keys), "jev", &file("good", "sk-its-own-file-0004", 0o600)), "set · file");
    // a value in the environment too short to keep out of the Log is not used either
    let name = "J4N_KEY_REFUSED_SHORT_KEY";
    // SAFETY: the name is this test's own; no other test reads or writes it
    unsafe { std::env::set_var(name, "ab12z") };
    assert_eq!(keys::state(None, "x", &format!("env:{name}")), "not used · too short");
    unsafe { std::env::remove_var(name) };
}

/// Its row in Agents › Providers says the J4n file that others can read is not used, and the provider is not usable for it.
#[test]
fn a_kept_key_others_can_read_is_said_not_used_on_its_provider_row() {
    let home = Scratch::new("key-refused-row");
    let kept = Keys::new(home.path());
    kept.set(&Redactor::new(), "gw", VALUE).unwrap();
    std::fs::set_permissions(home.join("keys/gw"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let f = fx_screened(Vec::new(), screen(), |c| c.keys = Some(kept.clone()));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let row = provider(&mut s, "gw");
    let texts: Vec<&str> = row.cells.iter().map(|c| c.text.as_str()).collect();
    assert!(texts.contains(&"not used · others can read the file") && !texts.contains(&"usable"), "{texts:?}");
}

/// A provider whose program is given the key J4n keeps for it (`kept:<provider>` in its `secrets`, as claude takes Anthropic's) has the
/// secret editor on its row, so the key can be typed, though its connector names no `key` of its own.
#[test]
fn a_provider_whose_program_takes_its_kept_key_has_the_key_editor_on_its_row() {
    const FILES: [(&str, &str); 6] = [
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("views/f1.toml", include_str!("../../../../packs/base/views/f1.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ("flows/signin.toml", include_str!("../../../../packs/base/flows/signin.toml")),
        ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
    ];
    let mut files: Vec<SourceFile> = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    files.push(SourceFile {
        path: "providers/acme.toml".into(),
        text: "api = 1\n[[provider]]\nid = \"acme\"\nfamily = \"subscription\"\nkey = \"env:J4N_KEPT_ROW_NOBODY_SETS_KEY\"\nconnectors = [\"acmecli\"]\n".into(),
    });
    files.push(SourceFile {
        path: "connectors/acmecli.toml".into(),
        text: "api = 1\n[[connector]]\nid = \"acmecli\"\nis = \"agent\"\ndialect = \"text\"\nprogram = \"acmecli\"\nstart = [\"{prompt}\"]\nsecrets = { ACME_API_KEY = \"kept:acme\" }\n".into(),
    });
    let (tables, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
    assert!(problems.is_empty(), "{problems:?}");
    let (settings, _) = Settings::load(&tables);
    let screen = every_provider(Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0);
    let home = Scratch::new("kept-row");
    let kept = Keys::new(home.path());
    let f = fx_screened(Vec::new(), screen, |c| c.keys = Some(kept.clone()));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let row = provider(&mut s, "acme");
    assert_eq!(row.data.clone().unwrap()["edit"], serde_json::json!({ "kind": "secret", "bind": "key:acme.value" }), "{row:?}");
    s.up(Up::Intent { intent: Intent::Key { id: "acme".into(), value: Secret::new(VALUE.into()) } });
    assert_eq!(std::fs::read_to_string(home.join("keys/acme")).unwrap(), VALUE, "kept for the provider, which `kept:acme` reads");
}

/// The base pack's pages and views, the commands, and a connector `gw` that takes a key from a name nobody sets.
fn screen() -> Screen {
    const FILES: [(&str, &str); 6] = [
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("views/f1.toml", include_str!("../../../../packs/base/views/f1.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ("flows/signin.toml", include_str!("../../../../packs/base/flows/signin.toml")),
        ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
    ];
    let mut files: Vec<SourceFile> = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    files.push(SourceFile {
        path: "connectors/gw.toml".into(),
        text: "api = 1\n[[connector]]\nid = \"gw\"\nis = \"agent\"\ndialect = \"openai\"\nurl = \"https://gw.example/v1\"\nkey = \"env:J4N_TS7B_NOBODY_SETS_KEY\"\n".into(),
    });
    let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
    let (settings, _) = Settings::load(&tables);
    Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
}

/// The screen with `tabs.all` on, so Agents › Providers lists the providers nobody set up yet (no connector, its program not here) too.
fn every_provider(mut screen: Screen) -> Screen {
    screen.set = Settings::from_pairs([("tabs.all", j4n_core::value::Value::Bool(true))]).tree();
    screen
}

fn provider(s: &mut Serve, key: &str) -> Row {
    let (downs, _) = s.up(Up::Watch { bind: "view:providers".into() });
    downs.into_iter().filter_map(|d| if let Down::Rows { rows, .. } = d { Some(rows) } else { None }).flatten().find(|r| r.key == key).unwrap_or_else(|| panic!("no row {key}"))
}

fn all_events(f: &Fx) -> String {
    f.rig.log.sync();
    f.rig.log.read(0).map(|e| format!("{} {}", e.kind, e.data)).collect::<Vec<_>>().join("\n")
}

#[test]
fn a_key_sent_from_its_row_is_kept_the_log_says_key_set_with_no_value_and_x_removes_it() {
    let home = Scratch::new("ts7b-front");
    let kept = Keys::new(home.path());
    let f = fx_screened(Vec::new(), screen(), |c| c.keys = Some(kept.clone()));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let row = provider(&mut s, "gw");
    let data = row.data.clone().unwrap();
    assert_eq!(data["edit"], serde_json::json!({ "kind": "secret", "bind": "key:gw.value" }), "the row carries the secret editor");
    assert!(row.cells.iter().any(|c| c.text == "not set"), "{row:?}");
    assert_eq!(row.actions.iter().map(|a| a.key.as_str()).collect::<Vec<_>>(), ["enter"], "no x while J4n keeps nothing");
    // ⏎ sent the value once, by its own path: every Down after it, and the Log, hold no part of it
    let (downs, _) = s.up(Up::Intent { intent: Intent::Key { id: "gw".into(), value: Secret::new(VALUE.into()) } });
    let mut seen = serde_json::to_string(&downs).unwrap();
    let later: Vec<Down> = f.rig.log.read(0).flat_map(|e| s.ev(&e)).collect();
    seen.push_str(&serde_json::to_string(&later).unwrap());
    assert!(!seen.contains(VALUE) && !seen.contains(&VALUE[..12]), "the screen is never sent the key");
    let log = all_events(&f);
    assert!(!log.contains(VALUE) && !log.contains(&VALUE[..12]), "the Log holds no part of the key:\n{log}");
    assert!(log.contains(r#"key.set {"connector":"gw"}"#), "{log}");
    assert!(!log.contains("edit.applied") && !log.contains("change.recorded") && !log.contains("proposal.made"), "not a Change, not a Grow row:\n{log}");
    assert_eq!(std::fs::read_to_string(home.join("keys/gw")).unwrap(), VALUE);
    assert_eq!(mode(&home.join("keys/gw")), 0o600);
    assert_eq!(f.rig.log.redactor().scrub(VALUE), "«secret:GW_KEY»", "the Log's redactor scrubs it");
    let row = provider(&mut s, "gw");
    assert!(row.cells.iter().any(|c| c.text == "set · J4n file"), "{row:?}");
    let x = row.actions.iter().find(|a| a.key == "x").expect("x while J4n keeps it");
    assert!(x.risk >= 60, "x asks on the card first");
    // x, after the card: the line it says removes the file and writes key.removed
    s.up(Up::Intent { intent: x.intent.clone() });
    assert!(!home.join("keys/gw").exists());
    let log = all_events(&f);
    assert!(log.contains(r#"key.removed {"connector":"gw"}"#), "{log}");
    assert!(provider(&mut s, "gw").cells.iter().any(|c| c.text == "not set"));
    // a key is never taken from a line: the refusal leads to the row instead
    s.up(Up::Intent { intent: Intent::Say { text: ":key gw sk-ts7b-typed-on-a-line-0042".into(), with: Vec::new() } });
    let log = all_events(&f);
    assert!(log.contains("never on a line"), "{log}");
    assert!(!log.contains("typed-on-a-line"), "a key typed on a line is not written: the Log keeps `:key …`\n{log}");
}

/// The base pack's pages and views, and a provider `acme` of a router no connector is on for, whose key comes from a name nobody sets.
fn screen_with_provider() -> Screen {
    const FILES: [(&str, &str); 6] = [
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("views/f1.toml", include_str!("../../../../packs/base/views/f1.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ("flows/signin.toml", include_str!("../../../../packs/base/flows/signin.toml")),
        ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
    ];
    let mut files: Vec<SourceFile> = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    files.push(SourceFile {
        path: "providers/acme.toml".into(),
        text: "api = 1\n[[provider]]\nid = \"acme\"\nfamily = \"router\"\nurl = \"https://acme.example/v1\"\nkey = \"env:J4N_J574_NOBODY_SETS_KEY\"\nhosts = [\"acme.example\"]\n".into(),
    });
    let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
    let (settings, _) = Settings::load(&tables);
    Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
}

#[test]
fn a_key_typed_on_a_provider_with_no_connector_is_kept_for_the_provider_and_any_connector_of_it_reads_it() {
    let home = Scratch::new("t574-front");
    let kept = Keys::new(home.path());
    let f = fx_screened(Vec::new(), every_provider(screen_with_provider()), |c| c.keys = Some(kept.clone()));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let row = provider(&mut s, "acme");
    let data = row.data.clone().unwrap();
    assert_eq!(data["edit"], serde_json::json!({ "kind": "secret", "bind": "key:acme.value" }), "a row with no connector takes the key");
    assert!(row.cells.iter().any(|c| c.text == "not set"), "{row:?}");
    s.up(Up::Intent { intent: Intent::Key { id: "acme".into(), value: Secret::new(VALUE.into()) } });
    assert_eq!(std::fs::read_to_string(home.join("keys/acme")).unwrap(), VALUE);
    let log = all_events(&f);
    assert!(log.contains(r#"key.set {"connector":"acme"}"#) && !log.contains(VALUE), "the Log names the provider and holds no value:\n{log}");
    let row = provider(&mut s, "acme");
    assert!(row.cells.iter().any(|c| c.text == "set · J4n file"), "{row:?}");
    assert!(row.actions.iter().any(|a| a.key == "x"), "x removes it");
    // a connector of the provider that comes later, with the same reference, finds the key as set
    let reference = "env:J4N_J574_NOBODY_SETS_KEY";
    assert_eq!(f.front.key_state("late", reference), "set · J4n file");
    assert_eq!(f.front.key_state("late", "env:J4N_J574_OTHER_NOBODY_KEY"), "not set", "a connector with another reference does not");
    // x takes it away
    let x = row.actions.iter().find(|a| a.key == "x").unwrap();
    s.up(Up::Intent { intent: x.intent.clone() });
    assert!(!home.join("keys/acme").exists());
    assert_eq!(f.front.key_state("late", reference), "not set");
}

/// A key typed on a `:key` line, in any case, said to a chat, or as `:key remove <it>`, reaches no Log line, no event
/// and no screen text; the line is kept as `:key …` and refused with the way to Agents › Providers. A value of no vendor's shape, so the
/// redactor's shapes would not have caught it.
#[test]
fn a_key_typed_on_a_key_line_is_in_no_log_line_event_or_screen_text() {
    let home = Scratch::new("t474-line");
    let f = fx_screened(Vec::new(), screen(), |c| c.keys = Some(Keys::new(home.path())));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let secret = format!("qz{nanos:09}{}vw", std::process::id());
    let mut seen = String::new();
    for line in [format!(":key own {secret}"), format!(":KEY gw {secret}"), format!("/key {secret}"), format!(":key remove {secret}"), format!("@c1 :key gw {secret}")] {
        let (downs, _) = s.up(Up::Intent { intent: Intent::Say { text: line.clone(), with: Vec::new() } });
        seen.push_str(&serde_json::to_string(&downs).unwrap());
    }
    let later: Vec<Down> = f.rig.log.read(0).flat_map(|e| s.ev(&e)).collect();
    seen.push_str(&serde_json::to_string(&later).unwrap());
    let log = all_events(&f);
    assert!(!log.contains(&secret) && !log.contains(&secret[..8]), "no Log line holds the key:\n{log}");
    assert!(!seen.contains(&secret) && !seen.contains(&secret[..8]), "no event or screen text holds the key");
    assert_eq!(log.matches(r#"chat.say {"text":":key …"}"#).count(), 5, "each line is kept as the word only:\n{log}");
    assert!(log.contains("never on a line") && log.contains("no connector by that name takes a key"), "{log}");
    // the line that names a connector the screen has is kept as typed, and says J4n keeps no key for it
    s.up(Up::Intent { intent: Intent::Say { text: ":key remove gw".into(), with: Vec::new() } });
    let log = all_events(&f);
    assert!(log.contains(r#"chat.say {"text":":key remove gw"}"#) && log.contains("J4n keeps no key for gw"), "{log}");
}

/// The key's intent prints nothing of it: `Debug`, `Display` and JSON say `«secret»`, and an intent read from text is never a key.
#[test]
fn an_intent_key_holds_no_character_of_the_value_in_any_print_and_is_never_read_from_text() {
    // characters that no name of the intent, its fields or the id holds
    const TYPED: &str = "ZQXJ0987ZQXJ0987";
    let intent = Intent::Key { id: "gw".into(), value: Secret::new(TYPED.into()) };
    let up = Up::Intent { intent: intent.clone() };
    for shown in [format!("{intent:?}"), format!("{up:?}"), format!("{up:#?}"), serde_json::to_string(&up).unwrap(), Secret::new(TYPED.into()).to_string()] {
        assert!(!shown.chars().any(|c| TYPED.contains(c)), "a character of the key is in {shown}");
        assert!(shown.contains(j4n_proto::SECRET_SHOWN), "{shown}");
    }
    assert_eq!(Intent::from_json(&serde_json::json!({ "intent": "key", "id": "gw", "value": TYPED })), None, "a prop is no road for a key");
    assert!(serde_json::from_str::<Up>(&serde_json::to_string(&up).unwrap()).is_err(), "what was written is not read back as a key");
}

/// A key comes by `Intent::Key` only: a `Set` on `key:` keeps nothing, and its words name no part of the value. A connector beside the
/// provider that could not be given the key is said by name (and the reason names no file of J4n's folder), and a second key says `replaced`.
#[test]
fn a_set_on_a_key_keeps_nothing_and_a_sibling_that_could_not_be_given_the_key_is_said_by_name() {
    let home = Scratch::new("cs2-sibling");
    let kept = Keys::new(home.path());
    let f = fx_screened(Vec::new(), screen_with_sibling(), |c| c.keys = Some(kept.clone()));
    let mut s = Serve::new(f.front.clone());
    s.up(Up::Hello { api: j4n_proto::API });
    s.up(Up::Intent { intent: Intent::Set { kind: "key".into(), id: "acme".into(), field: "value".into(), value: VALUE.into() } });
    assert!(!home.join("keys/acme").exists(), "a Set is not a road for a key");
    let log = all_events(&f);
    assert!(!log.contains(&VALUE[..12]) && log.contains("never on a line"), "{log}");
    // the sibling's file cannot be written: a folder stands where it would be
    std::fs::create_dir_all(home.join("keys/acme-gw")).unwrap();
    s.up(Up::Intent { intent: Intent::Key { id: "acme".into(), value: Secret::new(VALUE.into()) } });
    assert_eq!(std::fs::read_to_string(home.join("keys/acme")).unwrap(), VALUE, "the provider's own key is kept");
    let log = all_events(&f);
    assert!(log.contains("the same key could not be kept for acme-gw"), "the sibling is said:\n{log}");
    assert!(!log.contains(&home.path().display().to_string()) && !log.contains(&VALUE[..12]), "no path of J4n's folder, no part of the key:\n{log}");
    s.up(Up::Intent { intent: Intent::Key { id: "acme".into(), value: Secret::new("sk-cs2-the-second-value-7d41".into()) } });
    assert!(all_events(&f).contains("the key of acme is replaced"), "{}", all_events(&f));
}

/// [`screen_with_provider`], and a connector of the provider that names the same reference.
fn screen_with_sibling() -> Screen {
    const FILES: [(&str, &str); 6] = [
        ("pages/main.toml", include_str!("../../../../packs/base/pages/main.toml")),
        ("views/main.toml", include_str!("../../../../packs/base/views/main.toml")),
        ("views/f1.toml", include_str!("../../../../packs/base/views/f1.toml")),
        ("commands/main.toml", include_str!("../../../../packs/base/commands/main.toml")),
        ("flows/signin.toml", include_str!("../../../../packs/base/flows/signin.toml")),
        ("keys/main.toml", include_str!("../../../../packs/base/keys/main.toml")),
    ];
    let mut files: Vec<SourceFile> = FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    files.push(SourceFile {
        path: "providers/acme.toml".into(),
        text: "api = 1\n[[provider]]\nid = \"acme\"\nfamily = \"router\"\nurl = \"https://acme.example/v1\"\nkey = \"env:J4N_CS2_NOBODY_SETS_KEY\"\nhosts = [\"acme.example\"]\n".into(),
    });
    files.push(SourceFile {
        path: "connectors/acme-gw.toml".into(),
        text: "api = 1\n[[connector]]\nid = \"acme-gw\"\nis = \"agent\"\ndialect = \"openai\"\nurl = \"https://acme.example/v1\"\nkey = \"env:J4N_CS2_NOBODY_SETS_KEY\"\n".into(),
    });
    let (tables, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "base", files)]);
    let (settings, _) = Settings::load(&tables);
    Screen::load(tables, &settings, Vec::new(), &BTreeMap::new()).0
}
