//! Model words: the match table in `packs/base/tests/model_words.toml` against `resolve`,
//! through the real front: a word becomes the doer's model in full or is refused before anything starts.

use crate::front_cases::{chatty, fx};
use j4n_core::front::models::{ModelLists, Resolved, resolve};
use j4n_core::front::{Cfg, Said};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn table() -> toml::Table {
    let text = include_str!("../../../../packs/base/tests/model_words.toml");
    text.parse().unwrap()
}

fn lists() -> BTreeMap<String, Vec<String>> {
    let t = table();
    t["lists"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect())).collect()
}

fn strs(v: &toml::Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn the_match_table_holds_for_every_word_of_every_real_list() {
    let (t, lists) = (table(), lists());
    let cases = t["case"].as_array().unwrap();
    assert!(cases.len() >= 30, "the table is at least 30 cases: {}", cases.len());
    for c in cases {
        let (word, list) = (c["word"].as_str().unwrap(), &lists[c["list"].as_str().unwrap()]);
        let got = resolve(word, list);
        if let Some(one) = c.get("one") {
            assert_eq!(got, Resolved::One(one.as_str().unwrap().into()), "{word} in {}", c["list"]);
        } else if let Some(many) = c.get("many") {
            assert_eq!(got, Resolved::Many(strs(many)), "{word} in {}", c["list"]);
        } else {
            let Resolved::None(near) = got else { panic!("{word} should match none: {got:?}") };
            assert_eq!(near.len(), 3.min(list.len()));
            let want = strs(&c["near"]);
            if want != ["?"] {
                assert_eq!(near, want, "{word}");
            }
        }
    }
}

#[test]
fn a_name_is_never_changed_by_how_it_is_written() {
    let list = &lists()["pi"];
    let full = "deepseek/deepseek-v4.1-flash-1017";
    for w in ["deepseek/deepseek-v4.1-flash-1017", "DEEPSEEK-V4.1-FLASH-1017", "deepseek v4 1 flash 1017", " deepseek..4.1 flash 1017 "] {
        assert_eq!(resolve(w, list), Resolved::One(full.into()), "{w:?}");
    }
    assert!(matches!(resolve("", list), Resolved::None(_)));
    assert!(matches!(resolve("   ", list), Resolved::None(_)));
    assert!(matches!(resolve("anything", &[]), Resolved::None(n) if n.is_empty()));
}

/// Lists by doer, and how often each was read again.
pub struct Held(BTreeMap<String, Vec<String>>, AtomicUsize, Option<(String, Vec<String>)>);

impl ModelLists for Held {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        Some(self.0.get(doer).cloned().unwrap_or_default())
    }

    fn refresh(&self, doer: &str) -> Option<Vec<String>> {
        self.1.fetch_add(1, Ordering::SeqCst);
        match &self.2 {
            Some((d, fresh)) if d == doer => Some(fresh.clone()),
            _ => self.models(doer),
        }
    }
}

pub fn held(fresh: Option<(&str, &[&str])>) -> (Arc<Held>, impl FnOnce(&mut Cfg)) {
    let h = Arc::new(Held(lists(), AtomicUsize::new(0), fresh.map(|(d, l)| (d.to_string(), l.iter().map(|s| s.to_string()).collect()))));
    let again = h.clone();
    (h, move |c: &mut Cfg| c.models = again)
}

#[test]
fn switch_to_pi_with_a_model_word_binds_pi_to_the_newest_match_and_the_notice_names_it_in_full() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("pi", &["a"]), chatty("claude", &["b"])], cfg);
    assert_eq!(f.say(":switch to pi deepseek 4.1 latest"), Said::Command("switch".into()));
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref()), ("pi", Some("deepseek/deepseek-v4.1-flash-1017")));
    assert!(f.notes().iter().any(|n| n.contains("deepseek/deepseek-v4.1-flash-1017")), "{:?}", f.notes());
    // the filler words are not model words, wherever they stand
    assert_eq!(f.say(":switch on claude with sonnet"), Said::Command("switch".into()));
    let b = f.front.binding("main").unwrap();
    assert_eq!((b.doer.as_str(), b.model.as_deref()), ("claude", Some("sonnet")));
    // and a model word alone is for the doer the thread has
    assert_eq!(f.say(":switch opus"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("opus"));
    assert!(matches!(f.say(":switch to"), Said::Refused(w) if w.contains("`to`")));
}

#[test]
fn a_word_that_matches_two_asks_which_the_newest_first_and_one_that_matches_none_is_refused_with_the_nearest_three() {
    let (_, cfg) = held(None);
    let f = fx(vec![chatty("pi", &["a"])], cfg);
    let Said::Refused(menu) = f.say(":switch pi claude 5") else { panic!("two models match") };
    assert!(menu.contains("1. anthropic/claude-fable-5 (newest, suggested)") && menu.contains("3. anthropic/claude-sonnet-5"), "{menu}");
    assert!(f.front.binding("main").is_none(), "nothing is bound while the choice stands");
    assert_eq!(f.say("3"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("anthropic/claude-sonnet-5"));
    // the choice is taken once: a number after it is a line like any other
    assert!(f.front.binding("main").is_some());
    let Said::Refused(menu) = f.say(":switch pi gemini 2.5") else { panic!() };
    assert!(menu.contains("google/gemini-2.5-flash"), "{menu}");
    assert_eq!(f.say("google/gemini-2.5-pro"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("google/gemini-2.5-pro"));
    let Said::Refused(why) = f.say(":switch pi deepsek 9") else { panic!("none match") };
    assert!(why.contains("deepsek 9") && why.contains("nearest:"), "{why}");
    assert_eq!(why.split("nearest: ").nth(1).unwrap().split(", ").count(), 3, "{why}");
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("google/gemini-2.5-pro"), "a refused word changes nothing");
}

#[test]
fn a_word_the_doers_list_does_not_have_is_never_passed_on_and_a_new_model_is_found_by_reading_the_list_again() {
    let (h, cfg) = held(Some(("pi", &["acme/zeta-9", "acme/zeta-8"])));
    let f = fx(vec![chatty("pi", &["a"]), chatty("test", &["b"])], cfg);
    // pi has no `mistral`: refused, and the list was read once more before saying so
    let Said::Refused(_) = f.say(":switch pi mistral large") else { panic!() };
    assert_eq!(h.1.load(Ordering::SeqCst), 1);
    assert!(f.bound().is_empty(), "nothing was bound with the word");
    // a model the kept list lacks but a fresh read has
    assert_eq!(f.say(":switch pi zeta"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("acme/zeta-9"));
    // a doer that lists no models has nothing to match against: its word is its own
    assert_eq!(f.say(":switch test whatever"), Said::Command("switch".into()));
    assert_eq!(f.front.binding("main").unwrap().model.as_deref(), Some("whatever"));
}

#[test]
fn a_doers_own_full_id_is_passed_on_as_typed_and_a_made_up_one_is_still_refused() {
    use j4n_core::front::models::settle;
    let (h, _) = held(None);
    let one = |w: &str| settle(&*h, "claude", w);
    for w in ["claude-haiku-5-5", "CLAUDE-HAIKU-5-5", "claude-sonnet-4-5-20250929", "claude-opus-5"] {
        assert_eq!(one(w), Ok(Some(Resolved::One(w.to_lowercase()))), "{w}");
    }
    for w in ["claude-mistral-5-5", "claude-haiku-latest", "claude-haiku-5-x", "claude-haiku-", "pi-haiku-5-5", "haiku-5-5"] {
        assert!(one(w).is_err(), "{w}: {:?}", one(w));
    }
    // the family word still settles as it did
    assert_eq!(one("haiku"), Ok(Some(Resolved::One("haiku".into()))));
}
