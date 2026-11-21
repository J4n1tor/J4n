//! Which host serves a model: the `model` rows, the filters before anything is picked, the cooling of a
//! host that hit its limits, what the Log says of each host and each thread's cache, and what a move costs.

use crate::bench::{Bench, base_layer};
use crate::mock::{Mock, Say};
use crate::world::Scratch;
use j4n_connect::hosts::{COOLING_WINDOW_MS, Election, HostBook, Models, Route, TTL_MS};
use j4n_connect::model::doer::{Config, Dialect, ModelDoer};
use j4n_connect::net::Net;
use j4n_core::answer::Egress;
use j4n_core::change::Actor;
use j4n_core::clock::{Clock, ManualClock};
use j4n_core::doer::Doer;
use j4n_core::fold::{Fold, Folds};
use j4n_core::front::hosts::{Hosts, NoRecord, Record, Standing, Want};
use j4n_core::gate::Verdict;
use j4n_core::log::{Log, New, Opts, Scope};
use j4n_core::redact::Redactor;
use j4n_core::schema::DataClass;
use j4n_core::table::{Layer, LayerKind, SourceFile, Tables};
use j4n_core::task::{State, TaskSpec};
use j4n_core::value::{Key, Value};
use serde_json::{Value as Json, json};
use std::sync::Arc;

const ROWS: &str = r#"
api = 1

[[model]]
id = "oss"
connector = "or"
name = "openai/gpt-oss-120b"
default = true
endpoints = [
  { host = "Cerebras", quant = "fp16", zone = "any", uptime = 99.0, p_in = 0.35, p_out = 0.75, p_cache_read = 0.35 },
  { host = "DeepInfra", quant = "bf16", zone = "any", uptime = 99.5, p_in = 0.04, p_out = 0.19, p_cache_read = 0.01 },
  { host = "Sail", quant = "fp4", zone = "any", uptime = 99.0, p_in = 0.02, p_out = 0.10 },
  { host = "EuHost", quant = "bf16", zone = "eu", uptime = 99.0, p_in = 0.01, p_out = 0.05 },
  { host = "Flaky", quant = "bf16", zone = "any", uptime = 80.0, p_in = 0.005, p_out = 0.02 },
  { host = "Unpriced", zone = "any" },
]

[[model]]
id = "small"
connector = "or"
name = "vendor/small"
"#;

fn models() -> Models {
    let (t, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "m", vec![SourceFile { path: "m.toml".into(), text: ROWS.into() }])]);
    assert!(problems.is_empty(), "{problems:?}");
    let (m, problems) = Models::load(&t);
    assert!(problems.is_empty(), "{problems:?}");
    m
}

struct Eu;

impl Egress for Eu {
    fn check(&self, _: DataClass, zone: &str) -> Verdict {
        if zone == "eu" { Verdict::Block { why: "data stays out of the eu zone".into(), by: "t".into() } } else { Verdict::Allow { by: "t".into() } }
    }
}

struct Rig {
    dir: Scratch,
    log: Log,
    clock: Arc<ManualClock>,
    book: HostBook,
    #[allow(dead_code)]
    folds: Folds,
}

impl Rig {
    fn new() -> Rig {
        let dir = Scratch::new("hosts");
        let clock = Arc::new(ManualClock::at(1_000_000));
        let (log, problems) = Log::open(&dir.join(".j4n"), Scope::Project, Opts { segment_bytes: 8 << 20, max_line: 1 << 20, checkpoint_every: 1000, clock: clock.clone() }, Redactor::new()).unwrap();
        assert!(problems.is_empty());
        let book = HostBook::new();
        let folds = Folds::new(&dir.join("folds"));
        assert!(folds.add(Box::new(book.clone()), 0).is_empty());
        folds.attach(&log).unwrap();
        Rig { dir, log, clock, book, folds }
    }

    fn put(&self, kind: &str, thread: Option<&str>, data: Json) {
        let mut n = New::of(kind, Actor::Core, data);
        n.thread = thread.map(String::from);
        self.log.append(n).unwrap();
        self.log.sync();
    }

    fn limit(&self, host: &str, class: &str) {
        self.clock.advance(1_000);
        self.put("run.error", None, json!({ "text": "429 rate limit", "class": class, "endpoint": host, "model": "openai/gpt-oss-120b" }));
    }

    fn served(&self, thread: &str, host: &str, cached: u64) {
        self.clock.advance(1_000);
        self.put("run.served", Some(thread), json!({ "model": "openai/gpt-oss-120b", "host": host, "input": 40_000, "cached": cached, "wrote": 0, "output": 200, "ttft_ms": 400, "ms": 2_400 }));
    }

    fn election(&self, route: Route) -> Election {
        Election::new(models(), self.book.clone(), route, Some(Arc::new(Eu)))
    }
}

fn on() -> Route {
    Route { hosts: true, ..Route::default() }
}

// ---- the rows ----

#[test]
fn the_rows_load_and_a_connector_finds_its_model_by_name_id_or_default() {
    let m = models();
    assert_eq!(m.rows().len(), 2);
    assert_eq!(m.find("or", None).map(|r| r.id.as_str()), Some("oss"), "the default of the connector");
    assert_eq!(m.find("or", Some("openai/gpt-oss-120b")).map(|r| r.id.as_str()), Some("oss"));
    assert_eq!(m.find("or", Some("small")).map(|r| r.sent()), Some("vendor/small"));
    assert!(m.find("or", Some("nothing")).is_none() && m.find("other", None).is_none());
    assert_eq!(m.default_of("or"), Some("openai/gpt-oss-120b"));
    assert_eq!(m.default_of("other"), None);
    let bad = "api = 1\n[[model]]\nid = \"m\"\nconnector = \"or\"\nendpoints = [{ host = \"A\" }, { host = \"A\" }]\n[[model]]\nid = \"n\"\nconnector = \"or\"\nendpoints = [{ host = \"\" }]\n";
    let (t, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "m", vec![SourceFile { path: "m.toml".into(), text: bad.into() }])]);
    let (m, problems) = Models::load(&t);
    assert!(m.rows().is_empty());
    let said: Vec<String> = problems.iter().map(ToString::to_string).collect();
    assert!(said.iter().any(|s| s.contains("listed twice")) && said.iter().any(|s| s.contains("a name of one line")), "{said:?}");
}

// ---- the switch, and what is never offered ----

#[test]
fn with_the_switch_off_no_host_is_elected_and_with_it_on_the_cheapest_that_holds_the_filters_is() {
    let rig = Rig::new();
    assert!(rig.election(Route::default()).elect("or", None, &Want::default(), 0).is_none(), "off: the default endpoint");
    let on = rig.election(on());
    let pick = on.elect("or", None, &Want::default(), rig.clock.now_ms()).unwrap();
    // Cerebras 1.10, DeepInfra 0.23, Sail 0.12 are priced; EuHost is out by its zone, Flaky by its uptime, Unpriced goes last
    assert_eq!((pick.host.as_str(), pick.alts.clone()), ("Sail", vec!["DeepInfra".to_string(), "Cerebras".into(), "Unpriced".into()]));
    assert!(pick.why.contains("4 host(s)"), "{}", pick.why);
    // a model with no hosts listed, and a model nobody knows: the default endpoint
    assert!(on.elect("or", Some("small"), &Want::default(), 0).is_none());
    assert!(on.elect("or", Some("nothing"), &Want::default(), 0).is_none());
    assert!(on.elect("claude", None, &Want::default(), 0).is_none());
}

#[test]
fn a_host_whose_zone_fails_egress_or_whose_precision_is_refused_or_that_is_too_often_down_is_never_offered() {
    let rig = Rig::new();
    let all = |route: Route| -> Vec<String> {
        let e = rig.election(route);
        let p = e.elect("or", None, &Want::default(), rig.clock.now_ms()).unwrap();
        std::iter::once(p.host).chain(p.alts).collect()
    };
    let base = all(on());
    assert!(!base.contains(&"EuHost".to_string()), "its zone is blocked for this data: {base:?}");
    assert!(!base.contains(&"Flaky".to_string()), "its uptime 80% is under the floor: {base:?}");
    assert!(base.contains(&"Sail".to_string()));
    let no_fp4 = all(Route { quant: "no-fp4".into(), ..on() });
    assert!(!no_fp4.contains(&"Sail".to_string()) && no_fp4.contains(&"Cerebras".to_string()), "{no_fp4:?}");
    let none16 = all(Route { quant: "no-fp4, no-fp16".into(), ..on() });
    assert!(!none16.contains(&"Cerebras".to_string()) && !none16.contains(&"Sail".to_string()) && none16.contains(&"DeepInfra".to_string()), "{none16:?}");
    assert_eq!(all(Route { quant: "NO-FP4".into(), ..on() }), no_fp4, "the word is not case sensitive");
    assert!(all(Route { uptime_floor: 70.0, ..on() }).contains(&"Flaky".to_string()));
    assert!(all(Route { uptime_floor: 99.2, ..on() }).iter().all(|h| h == "DeepInfra" || h == "Unpriced"), "{:?}", all(Route { uptime_floor: 99.2, ..on() }));
    // a host that says nothing of its precision or its uptime is not refused for it
    let few = rig.election(Route { quant: "no-fp4,no-fp16,no-bf16".into(), uptime_floor: 100.0, ..on() });
    let p = few.elect("or", None, &Want::default(), 0).unwrap();
    assert_eq!((p.host.as_str(), p.alts.len()), ("Unpriced", 0));
}

// ---- a burst of limits ----

#[test]
fn a_host_that_returns_limits_in_a_burst_is_cooling_and_not_offered_until_it_cools() {
    let rig = Rig::new();
    let e = rig.election(on());
    let offered = |e: &Election| {
        let p = e.elect("or", None, &Want::default(), rig.clock.now_ms()).unwrap();
        std::iter::once(p.host).chain(p.alts).collect::<Vec<_>>()
    };
    assert_eq!(e.cooling("or", None, "Sail", rig.clock.now_ms()), None);
    rig.limit("Sail", "rate-limit");
    rig.limit("Sail", "transient");
    assert_eq!(e.cooling("or", None, "Sail", rig.clock.now_ms()), None, "two are not a burst");
    // an error that is the model's or the key's fault does not count
    rig.limit("Sail", "permanent");
    assert_eq!(rig.book.recent_errors("openai/gpt-oss-120b", "Sail", rig.clock.now_ms()), 2);
    rig.limit("Sail", "rate-limit");
    let why = e.cooling("or", None, "Sail", rig.clock.now_ms()).unwrap();
    assert!(why.contains("3 limits or errors"), "{why}");
    assert!(!offered(&e).contains(&"Sail".to_string()), "{:?}", offered(&e));
    assert_eq!(e.elect("or", None, &Want::default(), rig.clock.now_ms()).unwrap().host, "DeepInfra");
    // other hosts are not touched, and the count is per model
    assert_eq!(e.cooling("or", None, "DeepInfra", rig.clock.now_ms()), None);
    // five minutes later they are forgotten
    rig.clock.advance(COOLING_WINDOW_MS);
    assert_eq!(e.cooling("or", None, "Sail", rig.clock.now_ms()), None);
    assert_eq!(e.elect("or", None, &Want::default(), rig.clock.now_ms()).unwrap().host, "Sail");
    // the limit is a setting
    let strict = rig.election(Route { cooling: 1, ..on() });
    rig.limit("DeepInfra", "rate-limit");
    assert!(strict.cooling("or", None, "DeepInfra", rig.clock.now_ms()).is_some());
    assert!(e.cooling("or", None, "DeepInfra", rig.clock.now_ms()).is_none());
    // an error that names no host is no host's
    rig.put("run.error", None, json!({ "text": "x", "class": "rate-limit", "endpoint": null, "model": "openai/gpt-oss-120b" }));
    assert_eq!(rig.book.recent_errors("openai/gpt-oss-120b", "", rig.clock.now_ms()), 0);
}

// ---- what the Log says of the hosts ----

#[test]
fn what_a_host_served_and_what_it_cached_is_read_back_from_the_log() {
    let rig = Rig::new();
    rig.served("main", "Cerebras", 0);
    rig.served("main", "Cerebras", 38_000);
    rig.served("other", "Cerebras", 0);
    rig.served("main", "DeepInfra", 0);
    assert_eq!((rig.book.served("openai/gpt-oss-120b", "Cerebras"), rig.book.served("openai/gpt-oss-120b", "DeepInfra"), rig.book.served("openai/gpt-oss-120b", "Sail")), (3, 1, 0));
    let now = rig.clock.now_ms();
    // the cache of a thread is warm on the host that read it, until its time is up
    rig.served("main", "Cerebras", 38_000);
    let now = now + 1_000;
    assert_eq!(rig.book.warm("main", "Cerebras", TTL_MS, now), Some(38_000));
    assert_eq!(rig.book.warm("main", "Cerebras", TTL_MS, now + TTL_MS), None, "the time is up");
    assert_eq!(rig.book.warm("other", "Cerebras", TTL_MS, now), None, "a call that read nothing leaves nothing warm");
    assert_eq!(rig.book.warm("main", "DeepInfra", TTL_MS, now), None);
    assert_eq!(rig.book.warm("nobody", "Cerebras", TTL_MS, now), None);
    // what a rule or a view reads
    let read = |path: &[&str]| rig.book.read(&path.iter().map(|k| Key::Name((*k).into())).collect::<Vec<_>>(), rig.clock.now_ms());
    assert_eq!(read(&["hosts", "openai/gpt-oss-120b|Cerebras", "served"]), Value::num(4.0));
    assert_eq!(read(&["cache", "main|Cerebras", "cached"]), Value::num(38_000.0));
    let Value::Num(ttft) = read(&["hosts", "openai/gpt-oss-120b|Cerebras", "ttft_ms"]) else { panic!() };
    assert!((ttft - 400.0).abs() < 1e-6, "{ttft}");
    let Value::Num(tps) = read(&["hosts", "openai/gpt-oss-120b|Cerebras", "tps"]) else { panic!() };
    assert!((tps - 100.0).abs() < 1e-6, "200 tokens in the 2 s after the first word: {tps}");
    // a restart finds the same, from a snapshot and from the whole Log
    rig.folds.snapshot();
    let (a, b) = (HostBook::new(), HostBook::new());
    let snap = Folds::new(&rig.dir.join("folds"));
    assert!(snap.add(Box::new(a.clone()), rig.log.head()).is_empty());
    let full = Folds::new(&rig.dir.join("folds-full"));
    assert!(full.add(Box::new(b.clone()), 0).is_empty());
    full.attach(&rig.log).unwrap();
    assert_eq!(a.save(), rig.book.save());
    assert_eq!(b.save(), rig.book.save());
    // events it does not understand change nothing
    let before = rig.book.save();
    rig.put("run.served", Some("main"), json!({ "model": "m" }));
    rig.put("run.served", Some("main"), json!({ "host": "H" }));
    rig.put("run.metrics", Some("main"), json!({ "tokens_in": 4 }));
    assert_eq!(rig.book.save(), before);
}

#[test]
fn a_gateway_that_says_who_served_and_what_was_cached_is_read_back_by_the_folds_through_the_loop() {
    let gateway = Mock::start(|_, n| {
        let delta = json!({ "choices": [{ "delta": { "content": "ok" } }] }).to_string();
        let usage = json!({ "choices": [], "provider": "Cerebras", "usage": { "prompt_tokens": 1000, "completion_tokens": 10, "prompt_tokens_details": { "cached_tokens": if n == 0 { 0 } else { 900 } } } }).to_string();
        Say::Sse(vec![delta, usage])
    });
    let cfg = Config { id: "or".into(), dialect: Dialect::Openai, base: format!("{}/v1", gateway.url()), key: "k".into(), keyless: false, net: Net::new().with_env(|_| None), model: "openai/gpt-oss-120b".into(), system: String::new(), context: None, effort: None };
    let book = HostBook::new();
    let (tables, _) = Tables::load(vec![base_layer()]);
    let b = Bench::tuned(Scratch::new("bin"), vec![Arc::new(ModelDoer::new(cfg)) as Arc<dyn Doer>], &tables, {
        let book = book.clone();
        move |c| assert!(c.folds.add(Box::new(book), c.log.head()).is_empty())
    });
    for goal in ["one", "two"] {
        let mut spec = TaskSpec::new(goal, "or");
        spec.thread = Some("main".into());
        spec.tune.endpoint = Some("Cerebras".into());
        let id = b.tasks.spawn(spec, Actor::Operator).unwrap();
        assert!(matches!(b.end(&id).state, State::Done { ok: true }), "{}", b.dump());
    }
    b.log.sync();
    assert_eq!(book.served("openai/gpt-oss-120b", "Cerebras"), 2);
    let now = b.log.clock().now_ms();
    assert_eq!(book.warm("main", "Cerebras", TTL_MS, now), Some(900));
    // the host that was asked for is in the request, and the one that served is in the event
    assert_eq!(gateway.seen()[0].json()["provider"], json!({ "order": ["Cerebras"], "allow_fallbacks": false }));
    assert_eq!(b.events("run.served")[0].data["host"], "Cerebras");
}

// ---- a move, and what it costs ----

#[test]
fn another_host_is_the_next_that_holds_the_filters_and_the_cost_of_leaving_the_cache_is_said() {
    let rig = Rig::new();
    let e = rig.election(on());
    rig.served("main", "Cerebras", 40_000);
    let now = rig.clock.now_ms();
    let (pick, cost) = e.another("or", None, "main", "Cerebras", now).unwrap();
    assert_eq!(pick.host, "Sail", "the cheapest of what is left");
    // 40,000 tokens read at Sail's 0.02 instead of Cerebras's 0.35 for cache reads: the difference is the other way, so no loss in dollars
    assert!(cost.contains("40000 tokens"), "{cost}");
    let (pick, cost) = e.another("or", None, "main", "Sail", now).unwrap();
    assert_eq!(pick.host, "DeepInfra");
    assert!(cost.contains("Sail holds no cache for this thread"), "{cost}");
    // the dollars, when both prices are listed: DeepInfra's fresh price against Cerebras's cache price
    rig.served("t2", "DeepInfra", 100_000);
    let (_, cost) = e.another("or", None, "t2", "DeepInfra", rig.clock.now_ms()).unwrap();
    assert!(cost.contains("left behind: about $") && cost.contains("100000 tokens"), "{cost}");
    // a host that is cooling is not the next, and with none left there is no move
    for _ in 0..3 {
        rig.limit("Sail", "rate-limit");
        rig.limit("Cerebras", "rate-limit");
    }
    let (pick, _) = e.another("or", None, "main", "DeepInfra", rig.clock.now_ms()).unwrap_or_else(|| panic!("a host is left"));
    assert_eq!(pick.host, "Unpriced");
    for _ in 0..3 {
        rig.limit("Unpriced", "rate-limit");
    }
    assert!(e.another("or", None, "main", "DeepInfra", rig.clock.now_ms()).is_none());
    assert!(e.another("or", Some("small"), "main", "X", 0).is_none());
}

#[test]
fn the_cache_of_a_hundred_threads_is_kept_and_the_oldest_goes_first_past_the_limit() {
    let rig = Rig::new();
    for i in 0..258 {
        rig.served(&format!("thread-{i}"), "Cerebras", 1_000);
    }
    let now = rig.clock.now_ms();
    assert_eq!(rig.book.warm("thread-0", "Cerebras", u64::MAX, now), None, "the oldest was let go");
    assert_eq!(rig.book.warm("thread-1", "Cerebras", u64::MAX, now), None);
    assert_eq!(rig.book.warm("thread-2", "Cerebras", u64::MAX, now), Some(1_000));
    assert_eq!(rig.book.warm("thread-257", "Cerebras", u64::MAX, now), Some(1_000));
}

#[test]
fn a_limit_exactly_five_minutes_old_is_forgotten_and_one_a_moment_younger_is_not() {
    let rig = Rig::new();
    let e = rig.election(on());
    for _ in 0..3 {
        rig.put("run.error", None, json!({ "text": "429", "class": "rate-limit", "endpoint": "Sail", "model": "openai/gpt-oss-120b" }));
    }
    let t = rig.clock.now_ms();
    assert!(e.cooling("or", None, "Sail", t + COOLING_WINDOW_MS - 1).is_some());
    assert!(e.cooling("or", None, "Sail", t + COOLING_WINDOW_MS).is_none());
}

#[test]
fn an_unpriced_host_is_last_wherever_it_is_listed_and_a_negative_cache_gain_costs_nothing() {
    let rows = "api = 1\n[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"Unpriced\" }, { host = \"Dear\", p_in = 1.0, p_out = 1.0, p_cache_read = 5.0 }, { host = \"Cheap\", p_in = 0.1, p_out = 0.1 }]\n";
    let (t, _) = Tables::load(vec![Layer::new(LayerKind::Pack, "m", vec![SourceFile { path: "m.toml".into(), text: rows.into() }])]);
    let (models, problems) = Models::load(&t);
    assert!(problems.is_empty());
    let rig = Rig::new();
    let e = Election::new(models, rig.book.clone(), on(), None);
    let p = e.elect("c", None, &Want::default(), 0).unwrap();
    assert_eq!((p.host.as_str(), p.alts), ("Cheap", vec!["Dear".to_string(), "Unpriced".to_string()]));
    // leaving a cache whose reads cost more than a fresh prompt elsewhere loses nothing
    rig.clock.advance(1_000);
    rig.put("run.served", Some("main"), json!({ "model": "m", "host": "Dear", "input": 10, "cached": 5_000, "output": 1, "ms": 5 }));
    let (_, cost) = e.another("c", None, "main", "Dear", rig.clock.now_ms()).unwrap();
    assert!(cost.contains("about $0.0000"), "{cost}");
}

#[test]
fn a_call_that_ends_with_its_first_word_has_no_speed() {
    let rig = Rig::new();
    rig.clock.advance(1_000);
    rig.put("run.served", Some("t"), json!({ "model": "openai/gpt-oss-120b", "host": "Sail", "input": 10, "cached": 0, "output": 500, "ttft_ms": 400, "ms": 400 }));
    let read = |k: &str| rig.book.read(&["hosts", "openai/gpt-oss-120b|Sail", k].map(|k| Key::Name(k.into())), 0);
    assert_eq!(read("tps"), Value::num(0.0));
    assert_eq!(read("served"), Value::num(1.0));
}

#[test]
fn the_run_served_of_the_claude_doer_is_counted_like_the_own_agents_and_has_no_speed_to_invent() {
    let rig = Rig::new();
    rig.clock.advance(1_000);
    // what the claude dialect says: the full id and Anthropic, and nothing it was not told (no first word, no tokens)
    rig.put("run.served", Some("t"), json!({ "model": "claude-haiku-5-5", "host": "anthropic", "input": 0, "cached": 0, "wrote": 0, "output": 0, "ttft_ms": null, "ms": 0 }));
    let read = |k: &str| rig.book.read(&["hosts", "claude-haiku-5-5|anthropic", k].map(|k| Key::Name(k.into())), 0);
    assert_eq!((read("served"), read("tps")), (Value::num(1.0), Value::num(0.0)));
}

// ---- the pick weighs the task's own tokens, the price of waiting and the host's record ----

fn models_of(rows: &str) -> Models {
    let (t, problems) = Tables::load(vec![Layer::new(LayerKind::Pack, "m", vec![SourceFile { path: "m.toml".into(), text: format!("api = 1\n{rows}") }])]);
    assert!(problems.is_empty(), "{problems:?}");
    let (m, problems) = Models::load(&t);
    assert!(problems.is_empty(), "{problems:?}");
    m
}

/// What the Log says each role needs and each host has done, as a test says it.
#[derive(Default)]
struct Rec {
    mix: Vec<(&'static str, (u64, u64))>,
    results: Vec<(&'static str, &'static str, (u32, u32))>,
    outcomes: Vec<(&'static str, &'static str, Vec<bool>)>,
}

impl Record for Rec {
    fn results(&self, _: &str, host: &str, role: &str) -> (u32, u32) {
        self.results.iter().find(|(h, r, _)| *h == host && *r == role).map_or((0, 0), |(_, _, n)| *n)
    }

    fn mix(&self, role: &str) -> Option<(u64, u64)> {
        self.mix.iter().find(|(r, _)| *r == role).map(|(_, m)| *m)
    }

    fn outcomes(&self, _: &str, host: &str, role: &str) -> Vec<bool> {
        self.outcomes.iter().find(|(h, r, _)| *h == host && *r == role).map(|(_, _, o)| o.clone()).unwrap_or_default()
    }
}

fn want(role: &str) -> Want {
    Want { role: role.into(), ..Want::default() }
}

#[test]
fn the_pick_follows_the_shape_of_the_task_cheap_input_for_a_review_and_cheap_output_for_a_generation() {
    // one host sells input cheap and output dear, the other the reverse; both as fast as each other
    let m = models_of(
        "[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"CheapIn\", p_in = 0.05, p_out = 2.0, tps = 100.0, ttft_ms = 500 }, { host = \"CheapOut\", p_in = 0.5, p_out = 0.2, tps = 100.0, ttft_ms = 500 }]\n",
    );
    let rig = Rig::new();
    let mut e = Election::new(m, rig.book.clone(), Route { lambda: Some(0.0001), ..on() }, None);
    e.record = Arc::new(Rec { mix: vec![("gen", (2_500, 2_400)), ("review", (23_000, 400))], ..Rec::default() });
    let gen_ = e.elect("c", None, &want("gen"), 0).unwrap();
    let review = e.elect("c", None, &want("review"), 0).unwrap();
    assert_eq!((gen_.host.as_str(), review.host.as_str()), ("CheapOut", "CheapIn"), "output-heavy and input-heavy do not go to the same host");
    // the route row can say what it weighed
    assert!(gen_.why.contains("`gen`") && gen_.why.contains("λ $0.0001/min (pinned)"), "{}", gen_.why);
    assert_eq!(gen_.lines.len(), 2);
    assert!(gen_.lines[0].score.unwrap() < gen_.lines[1].score.unwrap());
}

#[test]
fn what_a_minute_is_worth_decides_between_fast_and_cheap_and_never_nothing_while_someone_waits() {
    let m = models_of(
        "[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"Cheap\", p_in = 0.04, p_out = 0.19, tps = 100.0, ttft_ms = 800 }, { host = \"Fast\", p_in = 0.35, p_out = 0.75, tps = 2000.0, ttft_ms = 200 }]\n",
    );
    let rig = Rig::new();
    let mut e = Election::new(m, rig.book.clone(), on(), None);
    e.record = Arc::new(Rec { mix: vec![("gen", (2_500, 2_400))], ..Rec::default() });
    let idle = e.elect("c", None, &want("gen"), 0).unwrap();
    assert_eq!(idle.host, "Cheap", "nobody waits: the base price of a minute");
    assert!(idle.why.contains("λ low: nobody waits"), "{}", idle.why);
    let waiting = e.elect("c", None, &Want { waiting: true, ..want("gen") }, 0).unwrap();
    assert_eq!(waiting.host, "Fast", "the operator waits: a minute is dear");
    assert!(waiting.why.contains("λ high: you are waiting"), "{}", waiting.why);
    assert!(waiting.lambda > idle.lambda * 2.9);
    // each reason raises it, and they add up; a pin beats them all
    let all = e.elect("c", None, &Want { waiting: true, blocked: true, tight: true, ..want("gen") }, 0).unwrap();
    assert!((all.lambda - idle.lambda * 12.0).abs() < 1e-9, "{} vs {}", all.lambda, idle.lambda);
    let pinned = e.elect("c", None, &Want { lambda: Some(0.0), waiting: true, ..want("gen") }, 0).unwrap();
    assert_eq!(pinned.host, "Cheap", "a time price of nothing is the cheapest and slowest, which only a pin chooses");
    // a weight below one cannot lower the price under the base
    let mut flat = Election::new(models_of("[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"A\", p_in = 1.0, p_out = 1.0 }]\n"), rig.book.clone(), Route { lambda_waiting: 0.0, ..on() }, None);
    flat.record = Arc::new(NoRecord);
    assert!(flat.elect("c", None, &Want { waiting: true, ..want("work") }, 0).unwrap().lambda >= 0.003);
}

#[test]
fn a_host_with_no_price_ranks_last_and_a_speed_that_was_not_measured_shows_a_tilde() {
    let m = models_of(
        "[[model]]\nid = \"m\"\nconnector = \"c\"\nname = \"v/m\"\ndefault = true\nendpoints = [{ host = \"Unpriced\", tps = 5000.0 }, { host = \"Listed\", p_in = 0.1, p_out = 0.4, tps = 80.0, ttft_ms = 600 }, { host = \"NoSpeed\", p_in = 0.1, p_out = 0.4 }]\n",
    );
    let rig = Rig::new();
    let e = Election::new(m, rig.book.clone(), on(), None);
    let p = e.elect("c", None, &want("work"), 0).unwrap();
    let hosts: Vec<&str> = p.lines.iter().map(|l| l.host.as_str()).collect();
    assert_eq!(hosts.last(), Some(&"Unpriced"), "{hosts:?}");
    assert!(p.lines.last().unwrap().score.is_none() && p.lines.last().unwrap().cost.is_none());
    let line = |h: &str| p.lines.iter().find(|l| l.host == h).unwrap();
    assert!(line("Listed").speed().starts_with('~'), "the listed speed is a guess: {}", line("Listed").speed());
    assert_eq!(line("NoSpeed").speed(), "—", "no speed at all");
    // a speed nobody knows is as slow as the slowest that is: it is never better than a host that has one
    assert!(line("NoSpeed").score.unwrap() >= line("Listed").score.unwrap());
    // once J4n has served a call at the host, its own measurement stands and the tilde goes
    rig.clock.advance(1_000);
    rig.put("run.served", Some("t"), json!({ "model": "v/m", "host": "Listed", "input": 100, "cached": 0, "output": 1_000, "ttft_ms": 500, "ms": 5_500 }));
    let p = e.elect("c", None, &want("work"), 0).unwrap();
    let listed = p.lines.iter().find(|l| l.host == "Listed").unwrap();
    assert!(!listed.speed().starts_with('~') && listed.measured, "{}", listed.speed());
}

#[test]
fn a_host_clearly_below_the_best_for_a_role_is_not_offered_and_one_with_no_record_gets_only_reading_first() {
    let m = models_of(
        "[[model]]\nid = \"m\"\nconnector = \"c\"\nname = \"v/m\"\ndefault = true\nendpoints = [{ host = \"Good\", p_in = 0.3, p_out = 0.3 }, { host = \"Poor\", p_in = 0.01, p_out = 0.01 }, { host = \"New\", p_in = 0.02, p_out = 0.02 }, { host = \"Middling\", p_in = 0.05, p_out = 0.05 }]\n",
    );
    let rig = Rig::new();
    let mut e = Election::new(m, rig.book.clone(), on(), None);
    e.record = Arc::new(Rec {
        results: vec![("Good", "work", (9, 1)), ("Poor", "work", (2, 8)), ("Middling", "work", (7, 3)), ("New", "work", (1, 0)), ("Poor", "review", (9, 1)), ("Good", "review", (9, 1))],
        ..Rec::default()
    });
    // the cheapest hosts are the ones with the worse record; the floor takes Poor out of `work` whatever it costs
    let work = e.elect("c", None, &want("work"), 0).unwrap();
    let names = |p: &j4n_core::front::hosts::Pick| p.lines.iter().map(|l| l.host.clone()).collect::<Vec<_>>();
    assert!(!names(&work).contains(&"Poor".to_string()), "{:?}", names(&work));
    assert!(!names(&work).contains(&"New".to_string()), "writing waits for a record: {:?}", names(&work));
    assert_eq!(work.host, "Middling", "the cheapest that holds the floor (0.7 is within 0.2 of 0.9)");
    // the same host has a good record for another role, and is offered there; a host with no record is offered tasks that only read
    let review = e.elect("c", None, &want("review"), 0).unwrap();
    assert!(names(&review).contains(&"Poor".to_string()), "a record is per role");
    let read = e.elect("c", None, &Want { mode: j4n_core::task::Mode::Read, ..want("work") }, 0).unwrap();
    assert!(names(&read).contains(&"New".to_string()) && !names(&read).contains(&"Poor".to_string()), "{:?}", names(&read));
    assert_eq!(read.lines.iter().find(|l| l.host == "New").unwrap().standing, Standing::Unknown);
    // when the floor would leave nothing, the hosts are still offered: a record never leaves a model with no host
    let mut all_poor = Election::new(models_of("[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"A\", p_in = 1.0, p_out = 1.0 }]\n"), rig.book.clone(), on(), None);
    all_poor.record = Arc::new(Rec { results: vec![("A", "work", (0, 9))], ..Rec::default() });
    assert_eq!(all_poor.elect("c", None, &want("work"), 0).unwrap().host, "A");
}

#[test]
fn the_time_price_has_a_floor_and_ignores_a_pin_that_is_not_a_price() {
    let m = models_of("[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"A\", p_in = 1.0, p_out = 1.0 }]\n");
    let rig = Rig::new();
    let e = Election::new(m, rig.book.clone(), Route { lambda_base: 0.0, ..on() }, None);
    assert!(e.elect("c", None, &want("work"), 0).unwrap().lambda > 0.0, "a minute is never worth nothing unless it is pinned to it");
    let rule = e.elect("c", None, &want("work"), 0).unwrap().lambda;
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        let p = e.elect("c", None, &Want { lambda: Some(bad), ..want("work") }, 0).unwrap();
        assert_eq!(p.lambda, rule, "{bad} is not a price");
    }
}

#[test]
fn a_record_counts_from_the_n_th_task_and_the_time_is_the_wait_to_the_first_word_plus_the_output_over_the_speed() {
    let m = models_of("[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"A\", p_in = 1.0, p_out = 1.0, tps = 100.0, ttft_ms = 1000 }, { host = \"B\", p_in = 0.5, p_out = 0.5, tps = 100.0, ttft_ms = 1000 }]\n");
    let rig = Rig::new();
    let mut e = Election::new(m, rig.book.clone(), Route { min_n: 5, ..on() }, None);
    e.record = Arc::new(Rec { mix: vec![("gen", (1_000, 2_000))], results: vec![("A", "gen", (5, 0)), ("B", "gen", (1, 4))], ..Rec::default() });
    let p = e.elect("c", None, &want("gen"), 0).unwrap();
    // A has exactly min_n results and all good; B has exactly min_n and 0.2 against 1.0: clearly below, not offered
    assert_eq!(p.lines.iter().map(|l| l.host.as_str()).collect::<Vec<_>>(), ["A"], "{:?}", p.lines);
    // 1 s to the first word, then 2,000 tokens at 100 a second
    assert!((p.lines[0].secs.unwrap() - 21.0).abs() < 1e-9, "{:?}", p.lines[0]);
    assert!((p.lines[0].cost.unwrap() - (1_000.0 * 1.0 + 2_000.0 * 1.0) / 1e6).abs() < 1e-12);
}

#[test]
fn without_a_record_a_plan_is_priced_for_twenty_thousand_tokens_in_and_two_and_a_half_out() {
    let m = models_of("[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"A\", p_in = 1.0, p_out = 10.0 }]\n");
    let rig = Rig::new();
    let e = Election::new(m, rig.book.clone(), on(), None);
    let c = e.elect("c", None, &want("plan"), 0).unwrap().lines[0].cost.unwrap();
    assert!((c - (20_000.0 + 25_000.0) / 1e6).abs() < 1e-12, "{c}");
}

#[test]
fn a_call_that_ended_with_its_first_word_gives_no_speed_to_measure() {
    let rig = Rig::new();
    rig.clock.advance(1_000);
    rig.put("run.served", Some("t"), json!({ "model": "m", "host": "H", "input": 10, "cached": 0, "output": 500, "ttft_ms": 400, "ms": 400 }));
    assert_eq!(rig.book.speed("m", "H"), None, "served, but nothing to say of its rate");
    rig.clock.advance(1_000);
    rig.put("run.served", Some("t"), json!({ "model": "m", "host": "H", "input": 10, "cached": 0, "output": 500, "ttft_ms": 400, "ms": 2_400 }));
    assert!(rig.book.speed("m", "H").is_some());
}

/// A plan that takes a hundredth of the list cost of every request of the connector `c`.
struct Hundredth;

impl j4n_core::front::hosts::PlanPrice for Hundredth {
    fn price(&self, doer: &str, usd: f64, _: u64) -> Option<f64> {
        (doer == "c").then_some(usd * 0.01)
    }
}

#[test]
fn a_doer_on_a_plan_is_weighed_at_what_a_request_takes_of_the_plan_so_time_counts_for_more() {
    let m = || {
        models_of(
            "[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"Cheap\", p_in = 0.04, p_out = 0.19, tps = 100.0, ttft_ms = 800 }, { host = \"Fast\", p_in = 0.35, p_out = 0.75, tps = 2000.0, ttft_ms = 200 }]\n",
        )
    };
    let rig = Rig::new();
    let mix = Rec { mix: vec![("gen", (2_500, 2_400))], ..Rec::default() };
    let mut listed = Election::new(m(), rig.book.clone(), on(), None);
    listed.record = Arc::new(Rec { mix: mix.mix.clone(), ..Rec::default() });
    let by_list = listed.elect("c", None, &want("gen"), 0).unwrap();
    assert_eq!(by_list.host, "Cheap", "at list price the saving is worth the wait");
    let mut planned = Election::new(m(), rig.book.clone(), on(), None);
    planned.record = Arc::new(Rec { mix: mix.mix.clone(), ..Rec::default() });
    planned.plan = Arc::new(Hundredth);
    let by_plan = planned.elect("c", None, &want("gen"), 0).unwrap();
    assert_eq!(by_plan.host, "Fast", "a request that takes a hundredth of its list cost saves nothing worth a wait: {}", by_plan.why);
    let cost = |p: &j4n_core::front::hosts::Pick, host: &str| p.lines.iter().find(|l| l.host == host).and_then(|l| l.cost).unwrap();
    assert!((cost(&by_plan, "Cheap") - cost(&by_list, "Cheap") * 0.01).abs() < 1e-12, "the cost weighed is the plan's");
    // a doer the plan does not name is weighed at list price
    let other = models_of("[[model]]\nid = \"m\"\nconnector = \"d\"\ndefault = true\nendpoints = [{ host = \"Cheap\", p_in = 0.04, p_out = 0.19, tps = 100.0, ttft_ms = 800 }, { host = \"Fast\", p_in = 0.35, p_out = 0.75, tps = 2000.0, ttft_ms = 200 }]\n");
    let mut elsewhere = Election::new(other, rig.book.clone(), on(), None);
    elsewhere.record = Arc::new(Rec { mix: mix.mix.clone(), ..Rec::default() });
    elsewhere.plan = Arc::new(Hundredth);
    assert_eq!(elsewhere.elect("d", None, &want("gen"), 0).unwrap().host, "Cheap");
}

#[test]
fn for_quality_the_host_with_the_best_record_comes_first_and_the_cost_only_orders_hosts_that_are_level() {
    // the dear host has passed every time; the cheap one passes and fails by turns; a third has no record at all
    let m = models_of(
        "[[model]]\nid = \"m\"\nconnector = \"c\"\ndefault = true\nendpoints = [{ host = \"Cheap\", p_in = 0.04, p_out = 0.19, tps = 100.0, ttft_ms = 500 }, { host = \"Dear\", p_in = 0.9, p_out = 2.0, tps = 100.0, ttft_ms = 500 }, { host = \"Blank\", p_in = 0.01, p_out = 0.05, tps = 100.0, ttft_ms = 500 }]\n",
    );
    let rig = Rig::new();
    let mut e = Election::new(m, rig.book.clone(), Route { lambda: Some(0.0001), ..on() }, None);
    e.record = Arc::new(Rec {
        mix: vec![("work", (2_500, 2_400))],
        outcomes: vec![("Dear", "work", vec![true; 6]), ("Cheap", "work", vec![true, false, true, false, true, false])],
        ..Rec::default()
    });
    let cost = e.elect("c", None, &want("work"), 0).unwrap();
    assert_eq!(cost.host, "Blank", "under cost the cheapest is first, whatever it has done: {}", cost.why);
    let quality = e.elect("c", None, &Want { quality: true, ..want("work") }, 0).unwrap();
    let order: Vec<&str> = std::iter::once(quality.host.as_str()).chain(quality.alts.iter().map(String::as_str)).collect();
    assert_eq!(order, ["Dear", "Cheap", "Blank"], "the best record, then the unsteady one, then the host with none: {}", quality.why);
    assert!(quality.why.contains("the best record of 3 host(s)") && !quality.why.contains("cheapest"), "{}", quality.why);
    // `another` (a move after a limit) is not a pick for quality: it stays the cheapest left
    let (other, _) = e.another("c", None, "t1", "Blank", 0).unwrap();
    assert_eq!(other.host, "Cheap");
}
