//! A setting is checked by its row as an edit is (its type, its range, its words), every
//! reader shares one live handle, `Cfg::apply` fills the front's fields from the settings as they are now, and a row says whether it waits
//! for the next start.

use crate::front_cases::{chatty, fx};
use j4n_core::branch::Switch;
use j4n_core::front::Cfg;
use j4n_core::front::screen::Screen;
use j4n_core::setting::{Live, SettingRow, Settings};
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::value::Value;
use serde_json::json;
use std::collections::BTreeMap;

const FILES: [(&str, &str); 8] = [
    ("settings/theme.toml", include_str!("../../../../packs/base/settings/theme.toml")),
    ("settings/lanes.toml", include_str!("../../../../packs/base/settings/lanes.toml")),
    ("settings/loop.toml", include_str!("../../../../packs/base/settings/loop.toml")),
    ("settings/route.toml", include_str!("../../../../packs/base/settings/route.toml")),
    ("settings/look.toml", include_str!("../../../../packs/base/settings/look.toml")),
    ("settings/cost.toml", include_str!("../../../../packs/base/settings/cost.toml")),
    ("settings/orchestrate.toml", include_str!("../../../../packs/base/settings/orchestrate.toml")),
    ("settings/crew.toml", include_str!("../../../../packs/crew/settings/crew.toml")),
];

fn tables(home: &str) -> Tables {
    let base = Layer::new(LayerKind::Pack, "base", FILES.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect());
    let mine = Layer::new(LayerKind::User, "home", vec![SourceFile { path: "mine.toml".into(), text: format!("api = 1\n{home}") }]);
    let (t, found) = Tables::load(vec![base, mine]);
    assert!(found.is_empty(), "{found:?}");
    t
}

fn row(t: &Tables, id: &str) -> SettingRow {
    t.get::<SettingRow>(id).unwrap()
}

#[test]
fn an_edit_of_a_setting_is_checked_by_its_type_its_range_and_its_words() {
    let t = tables("");
    let lanes = row(&t, "lanes");
    assert_eq!(lanes.words.as_deref(), Some(&["single".to_string(), "auto".into(), "wide".into(), "orchestrate".into()][..]), "an enum row has its words (trap 6)");
    assert_eq!(lanes.value_of(&json!("auto")), Ok(toml::Value::String("auto".into())));
    assert!(lanes.value_of(&json!("wider")).unwrap_err().contains("single, auto, wide"), "a word that is none of them is refused and they are named");
    let at = row(&t, "compact.at");
    assert_eq!(at.value_of(&json!(0.5)), Ok(toml::Value::Float(0.5)));
    assert_eq!(at.value_of(&json!("0.5")), Ok(toml::Value::Float(0.5)), "a number typed as words is a number");
    assert!(at.value_of(&json!(3.0)).unwrap_err().contains("above"), "out of range");
    assert!(at.value_of(&json!(true)).is_err(), "not a number");
    let chats = row(&t, "chats.on");
    assert_eq!(chats.value_of(&json!("on")), Ok(toml::Value::Boolean(true)));
    assert!(!chats.is_live(), "chats.on is wired at the start: it waits for the next one");
    assert!(lanes.is_live(), "a row that does not say is live");
    assert_eq!(row(&t, "cool.max").value_of(&json!("45m")), Ok(toml::Value::String("45m".into())));
    assert!(row(&t, "cool.max").value_of(&json!("soon")).is_err(), "a duration is checked as one");
}

#[test]
fn the_theme_words_of_the_rows_are_the_words_the_protocol_names() {
    let t = tables("");
    for (part, words) in j4n_proto::Look::PARTS.iter().filter(|(p, _)| *p != "motion") {
        let w = row(&t, &format!("theme.{part}")).words.unwrap_or_default();
        assert_eq!(w, words.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "theme.{part}");
    }
}

#[test]
fn one_live_handle_is_shared_and_an_edit_swaps_what_every_holder_reads() {
    let live = Live::new(Settings::load(&tables("")).0);
    let held = live.clone();
    assert_eq!(held.word("lanes", "?"), "single");
    live.set(Settings::load(&tables("[[setting]]\nid = \"lanes\"\nvalue = \"auto\"\n")).0);
    assert_eq!(held.word("lanes", "?"), "auto", "the clone reads the new settings");
    assert_eq!(held.now().tree.pointer("/lanes"), Value::text("auto"), "and so does the tree a rule reads");
}

#[test]
fn cfg_apply_fills_the_fields_from_the_settings_as_they_are_now_and_leaves_what_is_wired_at_the_start() {
    let t = tables("[[setting]]\nid = \"lanes\"\nvalue = \"wide\"\n[[setting]]\nid = \"session.mode\"\nvalue = \"each\"\n[[setting]]\nid = \"plan.doer\"\nvalue = \"codex\"\n[[setting]]\nid = \"chats.on\"\nvalue = true\n");
    let mut cfg = Cfg::new("p");
    cfg.chats.on = false;
    let notices = cfg.apply(&Settings::load(&t).0, &t);
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(cfg.lanes.word(), "wide");
    assert_eq!(cfg.session.word(), "each");
    assert_eq!(cfg.planner.doer.as_deref(), Some("codex"));
    assert!(!cfg.chats.on, "chats.on is wired at the start: apply does not turn it on");
    // a word the code does not know stands as the default, with a notice
    let bad = tables("[[setting]]\nid = \"session.mode\"\nvalue = \"sometimes\"\n");
    assert!(cfg.apply(&Settings::load(&bad).0, &bad).iter().any(|n| n.contains("auto stands")));
}

#[test]
fn a_setting_row_says_whether_it_waits_for_the_next_start() {
    let start = tables("");
    let (screen, _) = Screen::load(start.clone(), &Settings::load(&start).0, Vec::new(), &BTreeMap::new());
    let now = tables("[[setting]]\nid = \"chats.on\"\nvalue = true\n[[setting]]\nid = \"lanes\"\nvalue = \"auto\"\n");
    let edited = screen.with_settings(&Settings::load(&now).0, now);
    let of = |id: &str| {
        edited.setting_rows().into_iter().find(|r| r.pointer("/id") == Value::text(id)).unwrap()
    };
    assert_eq!((of("chats.on").pointer("/next_start"), of("chats.on").pointer("/waits")), (Value::Bool(true), Value::Bool(true)), "changed, and wired at the start");
    assert_eq!((of("lanes").pointer("/next_start"), of("lanes").pointer("/waits")), (Value::Bool(false), Value::Bool(false)), "live: it waits for nothing");
    assert_eq!(of("lanes").pointer("/now"), Value::text("auto"), "the screen's settings are the edited ones (trap 4)");
    assert_eq!(edited.waiting(), vec!["chats.on".to_string()], "what the restart card names");
    assert!(screen.waiting().is_empty());
}

/// The three sub-settings of the mode, as `Settings` read them, in a home that says `rows`.
fn mode(rows: &str) -> (bool, bool, bool) {
    let t = tables(rows);
    let (s, found) = Settings::load(&t);
    assert!(found.is_empty(), "{found:?}");
    (s.flag("orchestrate.look", false), s.flag("crew.on", false), s.flag("crew.envoys", false))
}

const WORD: &str = "[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n";

fn set(id: &str, value: &str) -> String {
    format!("[[setting]]\nid = \"{id}\"\nvalue = {value}\n")
}

#[test]
fn s_os1_the_word_orchestrate_alone_turns_the_look_the_crew_and_the_envoys_on_and_single_leaves_them_off() {
    assert_eq!(mode(""), (false, false, false), "single, nothing said: off, as before");
    assert_eq!(mode(WORD), (true, true, true), "only the word: every one follows it");
    assert_eq!(mode("[[setting]]\nid = \"lanes\"\nvalue = \"wide\"\n"), (false, false, false), "another word of lanes is not the switch");
    // the three are `auto` and say so, in the row and the screen
    let t = tables(WORD);
    for id in ["orchestrate.look", "crew.on", "crew.envoys"] {
        assert_eq!(row(&t, id).follows(), Some(("lanes", "orchestrate")), "{id}");
    }
    let (screen, _) = Screen::load(t.clone(), &Settings::load(&t).0, Vec::new(), &BTreeMap::new());
    let of = |id: &str| screen.setting_rows().into_iter().find(|r| r.pointer("/id") == Value::text(id)).unwrap().pointer("/now");
    assert_eq!((of("crew.on"), of("orchestrate.look")), (Value::text("auto"), Value::text("auto")), "the page says the word, not what it comes to");
}

#[test]
fn s_os1_a_hand_set_value_wins_over_auto_both_ways_and_a_true_or_false_of_an_older_file_reads_as_before() {
    for id in ["orchestrate.look", "crew.on", "crew.envoys"] {
        let pick = |word: &str, value: &str| {
            let (a, b, c) = mode(&format!("{word}{}", set(id, value)));
            match id {
                "orchestrate.look" => a,
                "crew.on" => b,
                _ => c,
            }
        };
        let (single, orchestrate) = ("", WORD);
        assert!(!pick(orchestrate, "false") && !pick(orchestrate, "\"off\""), "{id}: off by hand under the word stays off");
        assert!(pick(single, "true") && pick(single, "\"on\""), "{id}: on by hand under single stays on");
        assert!(pick(orchestrate, "true") && pick(orchestrate, "\"auto\"") && !pick(single, "\"auto\"") && !pick(single, "false"), "{id}");
    }
    let t = tables(&format!("{WORD}{}", set("crew.envoys", "false")));
    let mut cfg = Cfg::new("p");
    cfg.apply(&Settings::load(&t).0, &t);
    assert_eq!((cfg.orchestrate.look, cfg.crew.on, cfg.crew.envoys), (true, true, false), "Cfg reads the same: the envoys stay off by hand");
}

#[test]
fn s_os1_an_edit_of_a_row_with_auto_takes_a_yes_a_no_or_auto_and_a_word_that_is_none_is_refused() {
    let t = tables("");
    let look = row(&t, "orchestrate.look");
    assert_eq!(look.value_of(&json!(true)), Ok(toml::Value::String("on".into())), "the key of the catalogue sends a bool");
    assert_eq!(look.value_of(&json!(false)), Ok(toml::Value::String("off".into())));
    assert_eq!(look.value_of(&json!("auto")), Ok(toml::Value::String("auto".into())));
    assert_eq!((look.value_of(&json!("true")), look.value_of(&json!("false"))), (Ok(toml::Value::String("on".into())), Ok(toml::Value::String("off".into()))), "the words a switch took before");
    assert!(look.value_of(&json!("maybe")).unwrap_err().contains("auto, on, off"));
    assert!(row(&t, "crew.goal").value_of(&json!(true)).is_err(), "a row without `auto` is still words only");
}

/// A front whose Cfg is filled from the settings of a home that says `rows`, as the start fills it, with a screen (chats shown) and the doer `claude`.
fn front_of(rows: &'static str) -> crate::front_cases::Fx {
    fx(vec![chatty("claude", &["a"])], move |c| {
        let t = tables(rows);
        c.apply(&Settings::load(&t).0, &t);
        c.doer = Some("claude".into());
        c.chats.screen = true;
    })
}

#[test]
fn s_os1_with_only_the_word_set_the_chats_main_the_look_the_crew_and_the_envoys_act() {
    let f = front_of(WORD);
    assert_eq!(f.front.cfg().lanes, Switch::Orchestrate);
    assert!(f.front.chats_on() && f.front.main_works(), "the chats and Main");
    let c = f.front.cfg();
    assert!(c.orchestrate.look && c.crew.on && c.crew.envoys, "the look, the crew and its envoys, with no other setting touched");
    assert!(f.front.envoys_on(), "an envoy may be opened (it still acts only under a charter that is signed)");
    // single: none of it, whatever the packs hold
    let f = front_of("");
    let c = f.front.cfg();
    assert!(!f.front.chats_on() && !f.front.main_works() && !c.orchestrate.look && !c.crew.on && !c.crew.envoys && !f.front.envoys_on());
    // the word with the look turned off by hand: Main works, the line is not looked at
    let f = front_of("[[setting]]\nid = \"lanes\"\nvalue = \"orchestrate\"\n[[setting]]\nid = \"orchestrate.look\"\nvalue = false\n");
    assert!(f.front.main_works() && !f.front.cfg().orchestrate.look && f.front.cfg().crew.on);
}

#[test]
fn s_os9_a_ready_row_is_checked_against_the_settings_there_are_and_the_values_they_take() {
    use j4n_core::pack::Manifest;
    let t = tables("");
    let manifest = |set: &str| Manifest::parse(&format!("api = 1\nid = \"p\"\nversion = \"0.0.0\"\nsummary = \"s\"\nstability = \"experimental\"\n[ready]\n[ready.set]\n{set}\n")).unwrap();
    let problems = |set: &str| manifest(set).ready.unwrap().problems("p", &t).into_iter().map(|p| p.msg).collect::<Vec<_>>();
    assert!(problems("lanes = \"orchestrate\"").is_empty());
    assert!(problems("nope = true")[0].contains("is not a setting"));
    assert!(problems("lanes = \"sideways\"")[0].contains("is not a word of lanes"));
    assert!(problems("lanes = 3")[0].contains("lanes"));
    let none = Manifest::parse("api = 1\nid = \"p\"\nversion = \"0.0.0\"\nsummary = \"s\"\nstability = \"experimental\"\n[ready]\n[ready.set]\n").unwrap();
    assert!(none.ready.unwrap().problems("p", &t)[0].msg.contains("names one to"), "a row that sets nothing is refused");
    assert!(manifest("lanes = \"orchestrate\"").ready.unwrap().default, "ready is on by default for a pack the operator enables");
}
