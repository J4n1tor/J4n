//! Tiers: which of a doer's models is cheap, standard and strong, found by price words and measured cost, never by asking a model; and what the
//! tiers fill where a setting is empty: the quick look's model and Main's. The models are fixture lists, the doers scripts.

use crate::front_cases::{Fx, chatty, fx};
use j4n_core::branch::Switch;
use j4n_core::front::models::ModelLists;
use j4n_core::front::tiers::{MIN_TOKENS, PRICES, Spent, Tier, parse, pick, price_of};
use j4n_core::front::Cfg;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

fn strings(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

fn model(list: &[&str], spent: &BTreeMap<String, Spent>, tier: Tier) -> Option<String> {
    pick(&strings(list), &parse(PRICES), spent, tier).map(|p| p.model)
}

const CLAUDE: [&str; 3] = ["haiku", "sonnet", "opus"];

#[test]
fn a_tier_is_found_by_price_words_and_never_picks_opus_or_fable() {
    let none = BTreeMap::new();
    // claude lists the families: cheap is haiku, standard sonnet, strong opus, which only a pin takes
    assert_eq!(model(&CLAUDE, &none, Tier::Cheap).as_deref(), Some("haiku"));
    assert_eq!(model(&CLAUDE, &none, Tier::Standard).as_deref(), Some("sonnet"));
    assert_eq!(model(&CLAUDE, &none, Tier::Strong).as_deref(), Some("opus"));
    // a list of only the dear families has no cheap and no standard tier
    assert_eq!(model(&["opus", "fable"], &none, Tier::Cheap), None);
    assert_eq!(model(&["claude-opus-5-5", "claude-fable-5-2"], &none, Tier::Standard), None);
    // full ids: the newest of the cheapest family
    let full = ["claude-opus-5-5", "claude-haiku-4-5", "claude-haiku-5-5", "claude-sonnet-5-5"];
    assert_eq!(model(&full, &none, Tier::Cheap).as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(model(&full, &none, Tier::Standard).as_deref(), Some("claude-sonnet-5-5"));
    // a gateway list: the cheapest priced, the newest date of it; the next price up is standard; a name no word prices has no tier, and `gemini` is not `mini`
    let gateway = ["google/gemini-2.5-ultra", "deepseek/deepseek-v4.1-flash", "deepseek/deepseek-v4.1-flash-1017", "deepseek/deepseek-v4.1-pro", "openai/gpt-oss-20b"];
    assert_eq!(model(&gateway, &none, Tier::Cheap).as_deref(), Some("deepseek/deepseek-v4.1-flash-1017"));
    assert_eq!(model(&gateway, &none, Tier::Standard).as_deref(), Some("deepseek/deepseek-v4.1-pro"));
    assert_eq!(price_of(&parse(PRICES), "google/gemini-2.5-ultra"), None);
    assert_eq!(price_of(&parse(PRICES), "openai/gpt-4o-mini"), Some(1.0));
    assert_eq!(model(&["google/gemini-2.5-ultra", "openai/gpt-oss-20b"], &none, Tier::Cheap), None);
    // the operator's words are the prices
    assert_eq!(pick(&strings(&CLAUDE), &parse("opus=1 sonnet=2 haiku=3"), &none, Tier::Cheap).map(|p| p.model).as_deref(), Some("sonnet"), "opus is never taken, so sonnet is the cheapest");
}

#[test]
fn measured_cost_decides_only_when_every_model_that_may_be_picked_has_run_enough() {
    let ran = |usd: f64, tokens: u64| Spent { usd, tokens };
    let big = MIN_TOKENS * 50;
    // cache-heavy haiku runs cost more per token than the sonnet runs did: the measure wins
    let both = BTreeMap::from([("claude-haiku-5-5".to_string(), ran(5.0, big)), ("claude-sonnet-5-5".to_string(), ran(2.0, big))]);
    let got = pick(&strings(&CLAUDE), &parse(PRICES), &both, Tier::Cheap).unwrap();
    assert_eq!((got.model.as_str(), got.measured), ("sonnet", true));
    // one of them has run too little to tell: the price words decide for both
    let few = BTreeMap::from([("claude-haiku-5-5".to_string(), ran(5.0, big)), ("claude-sonnet-5-5".to_string(), ran(0.1, MIN_TOKENS - 1))]);
    let got = pick(&strings(&CLAUDE), &parse(PRICES), &few, Tier::Cheap).unwrap();
    assert_eq!((got.model.as_str(), got.measured), ("haiku", false));
}

struct Lists(BTreeMap<String, Vec<String>>);

impl ModelLists for Lists {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        self.0.get(doer).cloned()
    }
}

fn lists(c: &mut Cfg, claude: &[&str]) {
    c.models = Arc::new(Lists(BTreeMap::from([("claude".to_string(), strings(claude))])));
}

/// The orchestrate mode with the quick look on and no cheap tier named: only the model lists say what is cheap.
fn looking(claude: &'static [&'static str]) -> impl FnOnce(&mut Cfg) {
    move |c| {
        c.doer = Some("claude".into());
        c.lanes = Switch::Orchestrate;
        c.chats.screen = true;
        c.orchestrate.look = true;
        lists(c, claude);
    }
}

fn looked(f: &Fx) -> Value {
    f.rig.of_kind("orchestrate.looked")[0].data.clone()
}

fn of_role(f: &Fx, role: &str) -> Vec<(String, Option<String>)> {
    f.tasks().into_iter().filter(|t| t.spec.role.as_deref() == Some(role)).map(|t| (t.spec.doer, t.spec.tune.model)).collect()
}

#[test]
fn with_no_cheap_tier_named_the_look_takes_the_cheapest_model_of_a_ready_doer() {
    // the look was refused, "no cheap tier is named", while claude was signed in and had haiku
    let f = fx(vec![chatty("claude", &["a", "b"])], looking(&CLAUDE));
    f.turn("fix the typo in the readme");
    assert_eq!(of_role(&f, "look"), [("claude".to_string(), Some("haiku".to_string()))], "{}", f.rig.dump());
    assert!(looked(&f)["error"].as_str().is_none_or(|e| !e.contains("no cheap tier")), "{}", looked(&f));
}

#[test]
fn a_tier_never_takes_opus_for_the_look_and_a_need_the_operator_went_on_without_takes_nothing() {
    let f = fx(vec![chatty("claude", &["a"])], looking(&["opus"]));
    f.turn("fix the typo in the readme");
    assert!(of_role(&f, "look").is_empty(), "{:?}", of_role(&f, "look"));
    assert!(looked(&f)["error"].as_str().is_some_and(|e| e.contains("no cheap tier")), "{}", looked(&f));
    let f = fx(vec![chatty("claude", &["a"])], |c| {
        looking(&CLAUDE)(c);
        c.waived = vec!["orchestrate".into()];
    });
    f.turn("fix the typo in the readme");
    assert!(of_role(&f, "look").is_empty());
    assert!(looked(&f)["error"].as_str().is_some_and(|e| e.contains("went on without")), "{}", looked(&f));
    // what the operator named wins over the tier
    let f = fx(vec![chatty("claude", &["a", "b"])], |c| {
        looking(&CLAUDE)(c);
        c.cheap.model = Some("sonnet".into());
    });
    f.turn("fix the typo in the readme");
    assert_eq!(of_role(&f, "look"), [("claude".to_string(), Some("sonnet".to_string()))]);
}

fn bound_models(f: &Fx) -> Vec<Value> {
    f.bound().into_iter().map(|e| e.data["model"].clone()).collect()
}

#[test]
fn main_under_orchestrate_is_elected_with_the_standard_tiers_model_and_not_the_clis_default() {
    // `model: null` ran the claude program's default, Opus, at $1.97 a turn
    let f = fx(vec![chatty("claude", &["done"])], |c| {
        looking(&CLAUDE)(c);
        c.orchestrate.look = false;
    });
    f.turn("fix the typo in the readme");
    assert_eq!(bound_models(&f), [Value::from("sonnet")], "{}", f.rig.dump());
    // a pinned model is the operator's: it stays
    let f = fx(vec![chatty("claude", &["done"])], |c| {
        looking(&CLAUDE)(c);
        c.orchestrate.look = false;
        c.model = Some("opus".into());
    });
    f.turn("fix the typo in the readme");
    assert_eq!(bound_models(&f), [Value::from("opus")]);
    // `single` is unchanged: no model, the doer's own default
    let f = fx(vec![chatty("claude", &["done"])], |c| {
        looking(&CLAUDE)(c);
        c.lanes = Switch::Single;
    });
    f.turn("fix the typo in the readme");
    assert_eq!(bound_models(&f), [Value::Null]);
}
