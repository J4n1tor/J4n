//! The model library and the providers: one row per model whatever the way it is had, and one per provider, built
//! from what J4n knows and never from a key's value.

use j4n_core::front::library::{self, Src, Use};
use j4n_core::front::screen::Screen;
use j4n_core::query::Coll;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::value::Value;
use j4n_core::view::{Sources, outer};
use std::collections::BTreeMap;

fn map(pairs: &[(&str, Value)]) -> Value {
    Value::Map(pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect())
}

fn list(items: &[&str]) -> Value {
    Value::List(items.iter().map(|i| Value::text(*i)).collect())
}

fn field(row: &Value, k: &str) -> String {
    match row {
        Value::Map(m) => match m.get(k) {
            Some(Value::Str(s)) => s.clone(),
            Some(Value::Num(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn row<'a>(rows: &'a [Value], id: &str) -> &'a Value {
    rows.iter().find(|r| field(r, "id") == id).unwrap_or_else(|| panic!("no row {id} in {:?}", rows.iter().map(|r| field(r, "id")).collect::<Vec<_>>()))
}

fn claude() -> Value {
    map(&[("id", Value::text("claude")), ("is", Value::text("agent")), ("dialect", Value::text("claude-stream")), ("models", list(&["haiku", "sonnet", "opus"]))])
}

fn light() -> Value {
    map(&[
        ("id", Value::text("light")),
        ("is", Value::text("model")),
        ("dialect", Value::text("openai")),
        ("url", Value::text("https://openrouter.ai/api/v1")),
        ("key", Value::text("env:OPENROUTER_API_KEY")),
        ("models", list(&["openai/gpt-oss-20b"])),
    ])
}

fn signed_in() -> BTreeMap<String, (String, String)> {
    BTreeMap::from([("claude".to_string(), ("found · signed in".to_string(), "good".to_string()))])
}

fn used(doer: &str, model: &str, ok: bool, at_ms: u64) -> Use {
    Use { doer: doer.into(), model: model.into(), ok: Some(ok), at_ms, wall_ms: 10_000, out: 500 }
}

#[test]
fn claude_models_are_shown_with_no_router_key_and_say_the_account_and_plan_they_come_through() {
    let src = Src {
        connectors: vec![claude()],
        accounts: vec![map(&[("id", Value::text("claude-1")), ("connector", Value::text("claude")), ("plan", Value::text("max5")), ("state", Value::text("signed in"))])],
        plans: vec![map(&[("id", Value::text("max5")), ("connector", Value::text("claude")), ("price", Value::usd(100.0))])],
        doers: signed_in(),
        ..Src::default()
    };
    let rows = library::rows(&src);
    assert_eq!(rows.iter().map(|r| field(r, "id")).collect::<Vec<_>>(), ["haiku", "opus", "sonnet"]);
    let sonnet = row(&rows, "sonnet");
    assert_eq!((field(sonnet, "state").as_str(), field(sonnet, "via").as_str()), ("usable", "claude"));
    assert!(field(sonnet, "routes").contains("claude-1 max5"), "{sonnet:?}");
}

#[test]
fn a_model_that_is_not_signed_in_or_not_keyed_says_so() {
    let src = Src {
        connectors: vec![claude(), light()],
        doers: BTreeMap::from([("claude".to_string(), ("found · not signed in".to_string(), "warn".to_string()))]),
        keys: BTreeMap::from([("light".to_string(), "not set".to_string())]),
        ..Src::default()
    };
    let rows = library::rows(&src);
    assert_eq!(field(row(&rows, "sonnet"), "state"), "not signed in");
    assert_eq!(field(row(&rows, "gpt-oss-20b"), "state"), "no key");
}

#[test]
fn a_router_model_from_a_model_row_is_shown_with_its_hosts_price_and_window() {
    let model = map(&[
        ("id", Value::text("gpt-oss")),
        ("connector", Value::text("openrouter")),
        ("name", Value::text("openai/gpt-oss-120b")),
        ("context", Value::Num(131072.0)),
        ("endpoints", Value::List(vec![map(&[("host", Value::text("Cerebras")), ("p_in", Value::Num(0.35)), ("p_out", Value::Num(0.75))]), map(&[("host", Value::text("DeepInfra")), ("p_in", Value::Num(0.04)), ("p_out", Value::Num(0.19))])])),
    ]);
    let router = map(&[("id", Value::text("openrouter")), ("is", Value::text("model")), ("dialect", Value::text("openai")), ("key", Value::text("env:OPENROUTER_API_KEY"))]);
    let src = Src { connectors: vec![router], models: vec![model], keys: BTreeMap::from([("openrouter".to_string(), "set · env".to_string())]), ..Src::default() };
    let rows = library::rows(&src);
    let r = row(&rows, "gpt-oss-120b");
    assert_eq!((field(r, "state").as_str(), field(r, "context").as_str(), field(r, "price").as_str()), ("usable", "131072", "$0.04/$0.19 per M"));
}

#[test]
fn one_model_said_four_ways_is_one_row_with_its_routes() {
    let router = map(&[("id", Value::text("own")), ("is", Value::text("agent")), ("dialect", Value::text("openai")), ("key", Value::text("env:OPENROUTER_API_KEY")), ("models", list(&["anthropic/claude-sonnet-5.5"]))]);
    let src = Src {
        connectors: vec![claude(), router],
        doers: signed_in(),
        keys: BTreeMap::from([("own".to_string(), "set · env".to_string())]),
        uses: vec![used("claude", "claude-sonnet-5-5", true, 5_000), used("claude", "claude-sonnet-5-5[1m]", false, 9_000)],
        now_ms: 20_000,
        ..Src::default()
    };
    let rows = library::rows(&src);
    let sonnets: Vec<String> = rows.iter().map(|r| field(r, "id")).filter(|i| i.contains("sonnet")).collect();
    assert_eq!(sonnets, ["sonnet 5.5"], "{rows:?}");
    let r = row(&rows, "sonnet 5.5");
    assert_eq!(field(r, "via"), "claude, own");
    assert!(field(r, "used").contains("used 2 times, 1 of 2 done well") && field(r, "used").contains("also called sonnet"), "{}", field(r, "used"));
    assert!(matches!(r, Value::Map(m) if m.get("ago") == Some(&Value::Dur(11_000))));
    // a family nothing was seen of stays as the word
    assert!(rows.iter().any(|r| field(r, "id") == "haiku"));
}

#[test]
fn a_model_only_the_log_has_seen_is_shown() {
    let pi = map(&[("id", Value::text("pi")), ("is", Value::text("agent")), ("dialect", Value::text("pi-rpc"))]);
    let src = Src { connectors: vec![pi], uses: vec![used("pi", "deepseek/deepseek-v4", true, 1)], doers: BTreeMap::from([("pi".to_string(), ("found · signed in".to_string(), "good".to_string()))]), ..Src::default() };
    let rows = library::rows(&src);
    let r = row(&rows, "deepseek-v4");
    assert_eq!((field(r, "via").as_str(), field(r, "seen").as_str()), ("pi", "1"));
    // a use of a doer that no longer exists is kept, as seen
    let gone = Src { uses: vec![used("old", "gpt-4o", true, 1)], ..Src::default() };
    assert_eq!(field(row(&library::rows(&gone), "gpt-4o"), "state"), "seen");
}

const PROVIDERS: &[(&str, &str)] = &[
    ("providers/main.toml", include_str!("../../../../packs/base/providers/main.toml")),
];

fn provider_rows() -> Vec<Value> {
    let files = PROVIDERS.iter().map(|(p, t)| SourceFile { path: (*p).into(), text: (*t).into() }).collect();
    let (t, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "p", files)]);
    assert!(problems.is_empty(), "{problems:?}");
    t.rows("provider")
}

#[test]
fn the_providers_are_anthropic_openrouter_and_the_rest_with_the_state_of_each_key_and_never_a_value() {
    let provider_rows = provider_rows();
    assert_eq!(provider_rows.len(), 6, "the base pack names anthropic, openrouter, openai, google, typesafe and local");
    let pi = map(&[("id", Value::text("pi")), ("is", Value::text("agent")), ("dialect", Value::text("pi-rpc"))]);
    let src = Src {
        connectors: vec![claude(), pi],
        accounts: vec![map(&[("id", Value::text("claude-1")), ("connector", Value::text("claude")), ("plan", Value::text("max5")), ("state", Value::text("signed in"))])],
        doers: signed_in(),
        ..Src::default()
    };
    let keys = BTreeMap::from([("openrouter".to_string(), "set · env".to_string()), ("anthropic".to_string(), "not set".to_string())]);
    let rows = library::providers(&src, &provider_rows, &keys);
    let mut ids: Vec<String> = rows.iter().map(|r| field(r, "id")).collect();
    ids.sort();
    assert_eq!(ids, ["anthropic", "google", "local", "openai", "openrouter", "pi", "typesafe"]);
    let a = row(&rows, "anthropic");
    assert_eq!((field(a, "kind").as_str(), field(a, "state").as_str(), field(a, "accounts").as_str(), field(a, "models").as_str()), ("subscription", "usable", "claude-1 max5", "3"));
    let o = row(&rows, "openrouter");
    assert_eq!((field(o, "key").as_str(), field(o, "state").as_str()), ("set · env", "usable"));
    // with no key and no connector it is still listed, and says what is missing
    let none = library::providers(&src, &provider_rows, &BTreeMap::new());
    assert_eq!((field(row(&none, "openrouter"), "key").as_str(), field(row(&none, "openrouter"), "state").as_str()), ("not set", "no connector"));
    assert_eq!(field(row(&none, "pi"), "kind"), "harness");
}

#[test]
fn a_connector_that_speaks_a_router_s_api_is_that_providers_and_takes_the_key_typed_on_its_row() {
    let provider_rows = provider_rows();
    let src = Src { connectors: vec![light()], keys: BTreeMap::from([("light".to_string(), "not set".to_string())]), ..Src::default() };
    let rows = library::providers(&src, &provider_rows, &BTreeMap::from([("openrouter".to_string(), "not set".to_string())]));
    let o = row(&rows, "openrouter");
    assert_eq!((field(o, "through").as_str(), field(o, "keyid").as_str(), field(o, "switch").as_str(), field(o, "state").as_str()), ("light", "light", "light", "no key"));
}

struct Fake(BTreeMap<String, Vec<Value>>);

impl Sources for Fake {
    fn rows(&self, coll: &Coll) -> Vec<Value> {
        self.0.get(&coll.to_string()).cloned().unwrap_or_default()
    }
}

#[test]
fn the_models_and_providers_pages_read_the_library_and_not_the_connector_table() {
    let s = Screen::builtin();
    let src = Src { connectors: vec![claude()], doers: signed_in(), ..Src::default() };
    let fake = Fake(BTreeMap::from([("fold:library".to_string(), library::rows(&src)), ("fold:providers".to_string(), library::providers(&src, &provider_rows(), &BTreeMap::new()))]));
    let models = s.views.get("library").unwrap().rows(&fake, &outer(s.set.clone()));
    let mut names: Vec<&str> = models.iter().map(|r| r.cells[0].text.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["haiku", "opus", "sonnet"]);
    let providers = s.views.get("providers").unwrap().rows(&fake, &outer(s.set.clone()));
    assert_eq!(providers.len(), 6);
    assert_eq!(providers[0].cells[0].text, "anthropic");
}


#[test]
fn a_model_served_before_the_books_last_tasks_is_seen_by_the_fold_of_the_whole_log() {
    let src = Src {
        connectors: vec![claude()],
        doers: signed_in(),
        uses: vec![used("claude", "sonnet", true, 9_000), used("claude", "sonnet", true, 9_500)],
        // `claude-opus-4-1` is not in the Book any more; `claude-sonnet-5-5` is, twice, and the Log says it served nine times
        served: BTreeMap::from([("claude-opus-4-1".to_string(), (7, 3_000)), ("claude-sonnet-5-5".to_string(), (9, 9_400))]),
        now_ms: 20_000,
        ..Src::default()
    };
    let rows = library::rows(&src);
    let opus41 = row(&rows, "opus 4.1");
    assert_eq!((field(opus41, "seen").as_str(), field(opus41, "used").as_str()), ("7", "used 7 times · also called opus"), "the family word `opus` of claude's own list is it: {opus41:?}");
    assert!(matches!(opus41, Value::Map(m) if m.get("ago") == Some(&Value::Dur(17_000))), "when it last served, from the fold: {opus41:?}");
    // a model nothing names now is seen all the same
    let gone = library::rows(&Src { served: BTreeMap::from([("some/old-model".to_string(), (3, 100))]), now_ms: 400, ..Src::default() });
    assert_eq!((field(row(&gone, "old-model"), "state").as_str(), field(row(&gone, "old-model"), "seen").as_str()), ("seen", "3"));
    let sonnet = row(&rows, "sonnet 5.5");
    assert_eq!(field(sonnet, "seen"), "2", "a model the Book still holds keeps the Book's count, and the family word `sonnet` is it: {sonnet:?}");
    assert_eq!(rows.iter().filter(|r| field(r, "id").starts_with("sonnet")).count(), 1);
}

#[test]
fn a_list_no_connector_takes_is_the_providers_own_and_names_the_models_it_can_send() {
    let src = Src { loose: vec![("openrouter".into(), vec!["openai/gpt-oss-120b".into(), "anthropic/claude-sonnet-4.5".into()])], ..Src::default() };
    let rows = library::rows(&src);
    let gpt = row(&rows, "gpt-oss-120b");
    assert_eq!((field(gpt, "via").as_str(), field(gpt, "state").as_str()), ("openrouter", "no connector"), "{gpt:?}");
    let provider = provider_rows().into_iter().find(|p| field(p, "id") == "openrouter").unwrap();
    let out = library::providers(&src, &[provider], &BTreeMap::new());
    assert_eq!(field(row(&out, "openrouter"), "models"), "2", "the provider counts what its list says");
}

#[test]
fn the_cache_keeps_each_list_with_its_time_and_says_when_a_day_has_passed() {
    use j4n_core::listing::{Cache, DAY_MS, ROUTER};
    let home = crate::world::Scratch::new("t575-cache");
    let cache = Cache::new(home.path());
    assert!(cache.read().is_empty() && cache.due(ROUTER, 1), "nothing kept: due");
    cache.put(ROUTER, 1_000, &["a/one".into(), " a/one ".into(), "".into(), "b/two".into()]).unwrap();
    cache.put("pi", 2_000, &["x".into()]).unwrap();
    let kept = cache.read();
    assert_eq!(kept[ROUTER].models, ["a/one", "b/two"], "trimmed, no blank, no twice");
    assert_eq!((kept[ROUTER].at_ms, kept["pi"].at_ms), (1_000, 2_000), "each with its own time stamp");
    assert!(!cache.due(ROUTER, 1_000 + DAY_MS - 1) && cache.due(ROUTER, 1_000 + DAY_MS), "good for a day");
    std::fs::write(home.join("cache/models.json"), "not json").unwrap();
    assert!(cache.read().is_empty() && cache.due("pi", 5), "a cache that cannot be read is no cache, and is written over");
    cache.put("pi", 9, &["y".into()]).unwrap();
    assert_eq!(cache.read()["pi"].models, ["y"]);
}

#[test]
fn a_provider_with_no_connector_that_takes_a_key_takes_the_one_typed_on_its_row() {
    let provider_rows = provider_rows();
    let none = library::providers(&Src::default(), &provider_rows, &BTreeMap::new());
    assert_eq!(field(row(&none, "openrouter"), "keyid"), "openrouter", "the provider itself keeps the key");
    assert_eq!(field(row(&none, "local"), "keyid"), "", "a provider that takes no key keeps none");
    let src = Src { connectors: vec![light()], ..Src::default() };
    let with = library::providers(&src, &provider_rows, &BTreeMap::new());
    assert_eq!(field(row(&with, "openrouter"), "keyid"), "light", "with a connector the row is typed as it was");
}
