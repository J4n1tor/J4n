//! The keymap as data. The base pack lists the keys the screen reads; a layer above says other
//! keys do them, or none; a page's key sends an intent; the chords tmux takes are refused with their reason.

use j4n_core::front::keys::compile;
use j4n_core::front::page::Pages;
use j4n_core::table::{Layer, LayerKind, Level, Problem, SourceFile, Tables};
use j4n_proto::keys::{self, Code, Key};
use j4n_proto::{Bind, Intent};
use std::collections::BTreeMap;

const BASE_KEYS: &str = include_str!("../../../../packs/base/keys/main.toml");
const PAGE: &str = "api = 1\n[[page]]\nid = \"connectors\"\nparent = \"home\"\ntitle = \"Connectors\"\norder = 30\npresent = \"stack\"\n";

fn layer(kind: LayerKind, name: &str, files: &[(&str, &str)]) -> Layer {
    Layer::new(kind, name, files.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect())
}

fn base() -> Layer {
    layer(LayerKind::Pack, "base", &[("keys/main.toml", BASE_KEYS), ("pages/connectors.toml", PAGE)])
}

fn mine(text: &str) -> Layer {
    layer(LayerKind::User, "home", &[("ui/keys.toml", text)])
}

fn build(layers: Vec<Layer>) -> (Vec<Bind>, Vec<Problem>) {
    let (t, _) = Tables::load(layers);
    let (pages, _) = Pages::load(&t, &BTreeMap::new());
    let (binds, mut found) = compile(&t, &|p| pages.has(p));
    found.retain(|p| p.level == Level::Refused || p.at.starts_with("[[key]]"));
    (binds, found)
}

fn with(text: &str) -> (Vec<Bind>, Vec<Problem>) {
    build(vec![base(), mine(text)])
}

fn bind<'a>(binds: &'a [Bind], context: &str, key: &str) -> Option<&'a Bind> {
    binds.iter().find(|b| b.context == context && b.key == key)
}

fn to(binds: &[Bind], context: &str, key: &str) -> Option<String> {
    bind(binds, context, key).map(|b| b.to.clone())
}

fn said(found: &[Problem], at: &str, word: &str) -> bool {
    found.iter().any(|p| p.at == at && p.msg.contains(word))
}

fn key(s: &str) -> Key {
    s.parse().unwrap()
}

#[test]
fn the_base_pack_binds_every_key_the_screen_reads_to_itself_and_frees_none() {
    let (binds, found) = build(vec![base()]);
    assert!(found.is_empty(), "{found:?}");
    assert!(binds.len() >= 60, "{}", binds.len());
    assert!(binds.iter().all(|b| b.run.is_none() && b.to == b.key), "{:?}", binds.iter().find(|b| b.to != b.key));
    assert!(binds.windows(2).all(|w| (&w[0].context, &w[0].key) < (&w[1].context, &w[1].key)), "in one order, so that the same files tell the screen the same");
    for (context, k) in [("global", "C-k"), ("global", "M-1"), ("global", "M-0"), ("global", "F9"), ("composer", "S-Enter"), ("composer", "C-j"), ("transcript", "z"), ("list", "g"), ("list", "Space"), ("ask", "9"), ("ask", "?")] {
        assert!(bind(&binds, context, k).is_some(), "{context} {k}");
    }
}

#[test]
fn a_key_added_in_the_operators_folder_is_read_as_the_key_the_screen_knows() {
    let (binds, found) = with("api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"C-k\", \"C-p\"]\n");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!((to(&binds, "global", "C-p").as_deref(), to(&binds, "global", "C-k").as_deref()), (Some("C-k"), Some("C-k")));
}

#[test]
fn a_key_moved_frees_the_one_it_had_and_a_key_left_out_frees_the_row() {
    let (binds, found) = with("api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"C-p\"]\n[[key]]\nid = \"composer/aim\"\nkeys = []\n");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(to(&binds, "global", "C-p").as_deref(), Some("C-k"));
    for (context, k) in [("global", "C-k"), ("composer", "C-t")] {
        let b = bind(&binds, context, k).unwrap();
        assert!(b.to.is_empty() && b.run.is_none(), "{context} {k} is freed");
    }
}

#[test]
fn a_key_that_is_not_one_of_the_rows_native_keys_is_read_as_the_first() {
    let (binds, _) = with("api = 1\n[[key]]\nid = \"list/open\"\nkeys = [\"x\", \"Space\"]\n");
    // `x` stands for Enter, the first; Space is read as itself; Enter, which the screen reads and nobody binds now, is freed
    assert_eq!((to(&binds, "list", "x").as_deref(), to(&binds, "list", "Space").as_deref()), (Some("Enter"), Some("Space")));
    assert!(bind(&binds, "list", "Enter").is_some_and(|b| b.to.is_empty()));
}

#[test]
fn two_rows_that_bind_a_key_in_one_place_the_operators_wins_and_between_equals_the_first_keeps_it() {
    let (binds, found) = with("api = 1\n[[key]]\nid = \"list/up\"\nkeys = [\"Up\", \"Down\"]\n");
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(to(&binds, "list", "Down").as_deref(), Some("Up"), "the operator's row has Down");
    let (binds, _) = with("api = 1\n[[key]]\nid = \"list/down\"\nkeys = [\"Down\", \"Up\"]\n");
    assert_eq!(to(&binds, "list", "Up").as_deref(), Some("Down"), "and a pack's row, which comes after it by id, does not take the key back");
    let two = "api = 1\n[[key]]\nid = \"list/aa\"\nkeys = [\"w\"]\nnative = [\"Up\"]\n[[key]]\nid = \"list/bb\"\nkeys = [\"w\"]\nnative = [\"Down\"]\n";
    let (binds, found) = build(vec![layer(LayerKind::Pack, "p", &[("k.toml", two)])]);
    assert_eq!(to(&binds, "list", "w").as_deref(), Some("Up"));
    assert!(found.iter().any(|p| p.at == "[[key]] list/bb" && p.msg.contains("`list/aa`") && p.level == Level::Warn), "{found:?}");
}

#[test]
fn a_chord_tmux_takes_and_a_key_a_terminal_cannot_tell_apart_are_refused_with_the_reason() {
    let (binds, found) = with(
        "api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"C-k\", \"C-b\", \"M-Enter\", \"C-Space\", \"M-3\", \"C-M-Left\", \"M-Esc\", \"C-i\", \"C-m\", \"C-[\", \"C-q\"]\n",
    );
    let at = "[[key]] global/palette";
    for k in ["C-b", "M-Enter", "C-Space", "M-3", "C-M-Left", "M-Esc"] {
        assert!(found.iter().any(|p| p.at == at && p.msg.contains(&format!("`{k}`")) && p.msg.contains("tmux")), "{k}: {found:?}");
        // `M-3` stays the screen's own key of the third tab, and the palette does not get it
        assert_eq!(to(&binds, "global", k).as_deref(), (k == "M-3").then_some("M-3"), "{k}");
    }
    for (k, why) in [("C-i", "Tab"), ("C-m", "Enter"), ("C-[", "Esc")] {
        assert!(found.iter().any(|p| p.at == at && p.msg.contains(&format!("`{k}`")) && p.msg.contains(why)), "{k}: {found:?}");
    }
    assert_eq!(found.len(), 9, "{found:?}");
    assert_eq!((to(&binds, "global", "C-k").as_deref(), to(&binds, "global", "C-q").as_deref()), (Some("C-k"), Some("C-k")), "the rest stands");
}

#[test]
fn a_name_that_is_not_a_key_is_said_and_the_row_keeps_its_other_keys() {
    let (binds, found) = with("api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"Ctrl-x\", \"C-p\", \"F13\"]\n");
    assert!(said(&found, "[[key]] global/palette", "`Ctrl-x` is not a key"), "{found:?}");
    assert!(said(&found, "[[key]] global/palette", "`F13` is not a key"), "{found:?}");
    assert_eq!(to(&binds, "global", "C-p").as_deref(), Some("C-k"));
}

#[test]
fn a_row_that_cannot_stand_is_left_out_and_named() {
    let rows = "api = 1
[[key]]
id = \"nowhere/x\"
native = [\"x\"]
[[key]]
id = \"page:ghost/x\"
keys = [\"x\"]
run = { intent = \"run\", command = \"probe\" }
[[key]]
id = \"plain\"
native = [\"x\"]
[[key]]
id = \"global/\"
native = [\"x\"]
[[key]]
id = \"list/both\"
native = [\"x\"]
run = { intent = \"run\", command = \"probe\" }
[[key]]
id = \"list/neither\"
keys = [\"x\"]
[[key]]
id = \"page:connectors/own\"
native = [\"x\"]
[[key]]
id = \"page:connectors/odd\"
keys = [\"x\"]
run = { intent = \"fly\" }
[[key]]
id = \"list/badnative\"
native = [\"Hyper-x\"]
";
    let (binds, found) = with(rows);
    for (at, word) in [
        ("[[key]] nowhere/x", "`nowhere` is not a place"),
        ("[[key]] page:ghost/x", "`page:ghost` is not a place"),
        ("[[key]] plain", "`<context>/<name>`"),
        ("[[key]] global/", "`<context>/<name>`"),
        ("[[key]] list/both", "not both"),
        ("[[key]] list/neither", "needs `native`"),
        ("[[key]] page:connectors/own", "give `run`"),
        ("[[key]] page:connectors/odd", "not an intent"),
        ("[[key]] list/badnative", "not a key"),
    ] {
        assert!(said(&found, at, word), "{at}: {found:?}");
    }
    assert!(binds.iter().all(|b| b.key != "x"), "none of them binds anything");
}

#[test]
fn a_page_has_a_key_of_its_own_that_sends_an_intent() {
    let (binds, found) = with("api = 1\n[[key]]\nid = \"page:connectors/probe\"\nkeys = [\"r\"]\nrun = { intent = \"run\", command = \"probe\", args = [\"all\"] }\n");
    assert!(found.is_empty(), "{found:?}");
    let b = bind(&binds, "page:connectors", "r").expect("bound");
    assert_eq!((b.to.as_str(), b.run.clone()), ("", Some(Intent::Run { command: "probe".into(), args: vec!["all".into()] })));
}

#[test]
fn a_project_rebinds_nothing() {
    let theirs = layer(LayerKind::Project, "project", &[("ui/keys.toml", "api = 1\n[[key]]\nid = \"global/palette\"\nkeys = [\"C-p\"]\n")]);
    let (binds, _) = build(vec![base(), theirs]);
    assert_eq!(to(&binds, "global", "C-k").as_deref(), Some("C-k"));
    assert!(bind(&binds, "global", "C-p").is_none());
}

#[test]
fn the_names_of_keys_read_and_write_the_same_way() {
    for name in ["Enter", "Tab", "S-Tab", "Esc", "Up", "PgUp", "PgDn", "Home", "End", "Backspace", "Delete", "F1", "F12", "Space", "C-k", "M-Left", "C-M-Up", "S-Enter", "a", "G", "?", "-", "C--", "C-j", "[", "}"] {
        assert_eq!(key(name).to_string(), name, "{name}");
    }
    for (said, is) in [("enter", "Enter"), ("pageup", "PgUp"), ("Escape", "Esc"), ("BackTab", "S-Tab"), ("f5", "F5"), ("space", "Space"), ("S-a", "A"), ("S-G", "G"), ("C-S-M-x", "C-M-X")] {
        assert_eq!(key(said).to_string(), is, "{said}");
    }
    for bad in ["", "Ctrl", "C-", "F0", "F13", "ab", "C-Enterr", "Hyper-x"] {
        assert!(bad.parse::<Key>().is_err(), "{bad:?}");
    }
    assert!(key("a").types() && key("A").types() && !key("C-a").types() && !key("M-a").types() && !key("Enter").types());
    assert_eq!(key("S-Enter"), Key { code: Code::Enter, ctrl: false, alt: false, shift: true });
    for taken in ["M-1", "M-9", "M-S-Enter", "M-Enter", "M-Esc", "M-Up", "M-Down", "M-Left", "M-Right", "C-M-Right", "C-b", "C-Space", "C-i", "C-m", "C-["] {
        assert!(keys::refused(&key(taken)).is_some(), "{taken}");
    }
    for free in ["M-0", "M-a", "M-Backspace", "C-M-b", "C-M-Space", "C-Left", "C-Up", "C-Enter", "C-I", "C-M-i", "b", "Space", "S-Enter", "S-Tab", "Esc"] {
        assert!(keys::refused(&key(free)).is_none(), "{free}");
    }
    assert!(keys::refused(&key("C-j")).is_none() && keys::refused(&key("S-Enter")).is_none() && keys::refused(&key("C-Up")).is_none());
}

// ---- the keys row ----

mod hints {
    use super::*;
    use j4n_core::front::hints::rows;

    const BASE_HINTS: &str = include_str!("../../../../packs/base/keys/hints.toml");

    fn tables(mine: &str) -> Tables {
        let mut files = vec![("keys/main.toml", BASE_KEYS), ("keys/hints.toml", BASE_HINTS), ("pages/connectors.toml", PAGE)];
        if !mine.is_empty() {
            files.push(("keys/mine.toml", mine));
        }
        let (t, problems) = Tables::load(vec![layer(LayerKind::Pack, "base", &files)]);
        assert!(!problems.iter().any(|p| p.at.contains("[[hint]]")), "{problems:?}");
        t
    }

    fn view(mine: &str) -> Vec<(String, String, String)> {
        rows(&tables(mine)).0.into_iter().map(|r| (r.key, r.cells[0].text.clone(), r.cells[1].text.clone())).collect()
    }

    #[test]
    fn the_base_pack_hints_stand_in_the_order_of_their_order_with_the_keys_and_words_from_data() {
        let all = view("");
        let ids: Vec<&str> = all.iter().map(|(k, ..)| k.as_str()).collect();
        // the completion's hints (`ghost`, `step`) stand first only while they apply; of the rest the tab key stands first
        assert_eq!(ids.iter().find(|i| !i.starts_with("ghost/") && !i.starts_with("step/")), Some(&"global/tabs"), "the tab key stands first");
        assert!(ids.iter().position(|i| *i == "list/move") < ids.iter().position(|i| *i == "list/open"));
        assert!(ids.iter().position(|i| *i == "global/keys") < ids.iter().position(|i| *i == "global/find"), "{ids:?}");
        assert!(all.contains(&("table/find".into(), "/".into(), "find".into())));
        for log in ["move", "open", "find", "raw", "kind", "cause", "follow", "task", "copy", "all"] {
            assert!(ids.contains(&format!("log/{log}").as_str()), "the Log's key {log}");
        }
        for place in ["chat", "ask", "list", "form", "table", "tree", "cards", "log", "global"] {
            assert!(ids.iter().any(|i| i.starts_with(&format!("{place}/"))), "no hint for {place}");
        }
    }

    #[test]
    fn a_layer_rewords_reorders_or_hides_a_hint_by_its_id() {
        let mine = "api = 1\n[[hint]]\nid = \"list/open\"\nkeys = \"⏎\"\nword = \"enter\"\norder = 5\n[[hint]]\nid = \"page:connectors/probe\"\nkeys = \"r\"\nword = \"probe\"\norder = 15\n";
        let (t, _) = Tables::load(vec![layer(LayerKind::Pack, "base", &[("keys/main.toml", BASE_KEYS), ("keys/hints.toml", BASE_HINTS), ("pages/connectors.toml", PAGE)]), layer(LayerKind::User, "home", &[("ui/hints.toml", mine)])]);
        let all: Vec<_> = rows(&t).0.into_iter().map(|r| (r.key, r.cells[1].text.clone())).collect();
        let at = |id: &str| all.iter().position(|(k, _)| k == id);
        assert_eq!(all[at("list/open").unwrap()].1, "enter");
        assert!(at("list/open") < at("global/tabs"), "order 5 stands before 10");
        assert!(at("page:connectors/probe") < at("list/move"));
    }

    #[test]
    fn a_page_key_of_the_keys_table_with_a_word_is_a_hint_of_that_page_in_the_keys_that_do_it() {
        let mine = "api = 1\n[[key]]\nid = \"page:connectors/probe\"\ndoc = \"Probe.\"\nkeys = [\"C-p\", \"r\"]\nword = \"probe\"\nrun = { intent = \"run\", command = \"probe\" }\n[[key]]\nid = \"page:connectors/quiet\"\nkeys = [\"q\"]\nrun = { intent = \"run\", command = \"probe\" }\n";
        let all = view(mine);
        assert!(all.contains(&("page:connectors/probe".into(), "⌃p".into(), "probe".into())), "{all:?}");
        assert!(!all.iter().any(|(k, ..)| k.contains("quiet")), "a key with no word is not on the row");
    }

    #[test]
    fn a_hint_without_a_place_in_its_id_is_refused() {
        let (t, _) = Tables::load(vec![layer(LayerKind::Pack, "base", &[("keys/x.toml", "api = 1\n[[hint]]\nid = \"nowhere\"\nkeys = \"x\"\nword = \"y\"\n")])]);
        let (rows, found) = rows(&t);
        assert!(rows.is_empty() && found.iter().any(|p| p.msg.contains("<place>/<name>")), "{found:?}");
    }
}
