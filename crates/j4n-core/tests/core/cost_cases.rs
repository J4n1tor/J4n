//! A plan is a row with no price of its own, a window's money is the price over the windows in
//! a period, a request on a plan says what it cost the plan in the Log, the fold `plan_rate` measures what a full window is worth in list
//! dollars from the pairs of a window's moves and the cost spent between them, and before it has enough pairs the plan's assumption is used.

use crate::scripted::{Beat, Rig, Scripted, base_tables, end, load_base};
use j4n_core::change::Actor;
use j4n_core::cost::{Plan, PlanRate, Plans};
use j4n_core::fold::Fold;
use j4n_core::log::Ev;
use j4n_core::schema::DataClass;
use j4n_core::value::{Key, Value};
use j4n_core::task::TaskSpec;
use j4n_core::fold::declared;
use j4n_core::log::Scope;
use serde_json::{Value as Json, json};
use std::sync::Arc;

/// A row of the operator's is read on its own (it names the connector and the windows) and then laid over the base row of the same id.
const PRICE: &str = "[[plan]]\nid = \"claude\"\nconnector = \"claude\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\nprice = \"$24\"\nassume = \"$1000\"\n";
const NO_ASSUME: &str = "[[plan]]\nid = \"other\"\nconnector = \"other\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\nprice = \"$24\"\n";

fn plan(mine: &str) -> Plan {
    let (plans, problems) = Plans::load(&base_tables(mine));
    assert!(problems.is_empty(), "{problems:?}");
    plans.of("claude", None).expect("the base pack names the plan of claude").clone()
}

#[test]
fn the_base_pack_names_the_windows_and_leaves_the_price_to_the_operator() {
    let p = plan("");
    assert_eq!((p.price, p.money("seven_day"), p.money("five_hour")), (None, None, None));
    assert_eq!(p.assume, Some(1000.0), "what is assumed until the windows are measured");
    assert_eq!(p.connector, "claude");
}

#[test]
fn a_windows_money_is_the_price_over_the_windows_in_a_period() {
    let p = plan(PRICE);
    assert_eq!(p.price, Some(24.0));
    // $24 a month and 30.44 days: 4.35 weeks, $5.52 a week; five hours are 1/146 of the month
    assert!((p.money("seven_day").unwrap() - 24.0 * 7.0 / 30.44).abs() < 1e-9);
    assert!((p.money("five_hour").unwrap() - 24.0 * (5.0 / 24.0) / 30.44).abs() < 1e-9);
    assert_eq!(p.money("thirty_day"), None);
    let weekly = plan("[[plan]]\nid = \"claude\"\nconnector = \"claude\"\nwindows = [\"seven_day=7d\"]\nprice = \"$10\"\nperiod = \"week\"\n");
    assert!((weekly.money("seven_day").unwrap() - 10.0).abs() < 1e-9);
}

/// An account's row can name its plan (`plan = "pro"`), a plan row of the base pack (`max5`, `pro`) or the operator's own,
/// with the price the operator gives it; an account that names none, or one that names a plan nobody wrote, is paid by the connector's own.
#[test]
fn an_account_names_its_plan_and_the_connectors_own_plan_serves_every_other_request() {
    let mine = "[[plan]]\nid = \"pro\"\nconnector = \"claude\"\nwindows = [\"five_hour=5h\", \"seven_day=7d\"]\nprice = \"$25\"\n";
    let (mut plans, problems) = Plans::load(&base_tables(mine));
    assert!(problems.is_empty(), "{problems:?}");
    let before = plans.of("claude", None).map(|p| p.id.clone());
    assert_eq!(before.as_deref(), Some("claude"), "the tiers the base pack ships are nobody's default");
    let said = plans.name([("work".to_string(), "max5".to_string()), ("side".to_string(), "pro".to_string()), ("ghost".to_string(), "no-such".to_string())]);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].to_string().contains("no-such"), "{said:?}");
    let id = |a: Option<&str>| plans.of("claude", a).map(|p| p.id.clone());
    assert_eq!((id(Some("work")).as_deref(), id(Some("side")).as_deref()), (Some("max5"), Some("pro")));
    assert_eq!((id(Some("ghost")).as_deref(), id(Some("nobody")).as_deref(), id(None).as_deref()), (Some("claude"), Some("claude"), Some("claude")));
    let side = plans.of("claude", Some("side")).unwrap();
    assert_eq!(side.price, Some(25.0), "the operator's own row of the same id changes the tier's price");
    assert_eq!(plans.of("claude", Some("work")).unwrap().price, Some(100.0), "Max5 at the list price the operator gave");
    assert_eq!(plans.of("claude", None).unwrap().price, None, "the connector's own plan has none");
    assert_eq!(plans.of("pi", Some("work")), None, "a plan of claude does not pay for pi");
}

#[test]
fn a_plan_of_an_account_wins_for_that_account_and_the_plan_for_any_serves_the_rest() {
    let mine = "[[plan]]\nid = \"a-work\"\nconnector = \"claude\"\naccount = \"work\"\nprice = \"$100\"\nwindows = [\"seven_day=7d\"]\n";
    let (plans, problems) = Plans::load(&base_tables(mine));
    assert!(problems.is_empty(), "{problems:?}");
    let id = |c: &str, a: Option<&str>| plans.of(c, a).map(|p| p.id.clone());
    assert_eq!(id("claude", Some("work")).as_deref(), Some("a-work"));
    assert_eq!(id("claude", Some("home")).as_deref(), Some("claude"));
    assert_eq!(id("claude", None).as_deref(), Some("claude"));
    assert_eq!(id("pi", None), None, "a connector with no plan is paid by its key");
}

#[test]
fn a_plan_that_cannot_be_used_is_a_problem_and_no_plan() {
    for (row, word) in [
        ("price = \"$0\"", "more than zero"),
        ("windows = [\"seven_day\"]", "name=length"),
        ("windows = [\"seven_day=0s\"]", "name=length"),
        ("windows = []", "at least one"),
    ] {
        let mut fields = format!("id = \"bad\"\nconnector = \"x\"\nwindows = [\"seven_day=7d\"]\n{row}\n");
        if row.starts_with("windows") {
            fields = format!("id = \"bad\"\nconnector = \"x\"\n{row}\n");
        }
        let (plans, problems) = Plans::load(&base_tables(&format!("[[plan]]\n{fields}")));
        assert!(problems.iter().any(|p| p.at == "plan bad" && p.msg.contains(word)), "{row}: {problems:?}");
        assert!(plans.of("x", None).is_none(), "{row}");
    }
}

fn win_ev(window: &str, utilization: f64, resets_at: i64, account: &str) -> Ev {
    let mut data = json!({ "window": window, "utilization": utilization, "resets_at": resets_at, "status": "allowed" });
    if !account.is_empty() {
        data["account"] = json!(account);
    }
    ev("run.window", Actor::Doer("claude".into()), data)
}

fn ev(kind: &str, by: Actor, data: Json) -> Ev {
    Ev { seq: 0, at_ms: 0, kind: kind.into(), task: None, thread: None, by, cause: None, class: DataClass::default(), data }
}

fn spent(usd: f64, account: &str) -> Ev {
    ev("run.metrics", Actor::Doer("claude".into()), if account.is_empty() { json!({ "usd": usd }) } else { json!({ "usd": usd, "account": account }) })
}

/// What the fold says of `key` (`doer|account|window`): how many pairs and their median.
fn said(f: &PlanRate, key: &str) -> (f64, f64) {
    let at = |field: &str| match f.read(&[Key::Name(key.into())], 0) {
        Value::Map(m) => match m.get(field) {
            Some(Value::Num(n)) => *n,
            other => panic!("{field}: {other:?}"),
        },
        other => panic!("{key}: {other:?}"),
    };
    (at("n"), (at("median") * 1e6).round() / 1e6)
}

fn feed(events: Vec<Ev>) -> PlanRate {
    let mut f = PlanRate::default();
    for e in events.iter().filter(|e| PlanRate::default().wants(&e.kind)) {
        f.apply(e);
    }
    f
}

const FIVE: &str = "claude||five_hour";

#[test]
fn two_moves_of_a_window_make_a_pair_of_the_cost_between_them_and_the_first_only_marks_a_step() {
    let at = |u: f64| win_ev("five_hour", u, 100, "");
    // 0.00 → 0.01 → 0.02 with $0.14 spent between the moves: a pair of $14 for a full window
    let f = feed(vec![at(0.0), spent(0.07, ""), at(0.01), spent(0.14, ""), at(0.02)]);
    assert_eq!(said(&f, FIVE), (1.0, 14.0));
    // the first move has no pair: where in its step the window began is not known
    assert_eq!(said(&feed(vec![at(0.0), spent(0.07, ""), at(0.01)]), FIVE).0, 0.0);
    // a window that does not move gathers cost, and the pair is of all of it
    let f = feed(vec![at(0.0), at(0.01), spent(0.07, ""), at(0.01), spent(0.07, ""), at(0.01), at(0.02)]);
    assert_eq!(said(&f, FIVE), (1.0, 14.0));
    // a move of two steps is a pair of two steps
    assert_eq!(said(&feed(vec![at(0.0), at(0.01), spent(0.28, ""), at(0.03)]), FIVE), (1.0, 14.0));
}

#[test]
fn a_window_that_reset_fell_or_moved_with_nothing_spent_makes_no_pair() {
    let at = |u: f64, resets: i64| win_ev("five_hour", u, resets, "");
    let count = |events: Vec<Ev>| said(&feed(events), FIVE).0;
    assert_eq!(count(vec![at(0.0, 1), at(0.01, 1), spent(0.1, ""), at(0.02, 2)]), 0.0, "a new window is not the one that was measured");
    assert_eq!(count(vec![at(0.0, 1), at(0.02, 1), spent(0.1, ""), at(0.01, 1), at(0.02, 1)]), 0.0, "a fall without a reset starts again");
    assert_eq!(count(vec![at(0.0, 1), at(0.03, 1), spent(0.1, ""), at(0.01, 1), at(0.04, 1)]), 0.0, "and the move after it is the first of a new start");
    assert_eq!(count(vec![at(0.0, 1), at(0.01, 1), at(0.02, 1)]), 0.0, "a step with nothing spent in it says nothing");
    // after a reset the next two moves are a pair again
    assert_eq!(count(vec![at(0.5, 1), at(0.0, 2), at(0.01, 2), spent(0.14, ""), at(0.02, 2)]), 1.0);
}

#[test]
fn the_median_of_the_latest_pairs_is_what_a_window_is_worth() {
    let pair = |per: f64, from: f64| vec![spent(per / 100.0, ""), win_ev("five_hour", from + 0.01, 100, "")];
    let mut events = vec![win_ev("five_hour", 0.0, 100, ""), win_ev("five_hour", 0.01, 100, "")];
    // pairs of 10, 30 and 20: the middle one; of 10, 30, 20 and 40: the mean of the two in the middle
    for (i, per) in [10.0, 30.0, 20.0].into_iter().enumerate() {
        events.extend(pair(per, 0.01 + 0.01 * i as f64));
    }
    assert_eq!(said(&feed(events.clone()), FIVE), (3.0, 20.0));
    events.extend(pair(40.0, 0.04));
    assert_eq!(said(&feed(events), FIVE), (4.0, 25.0));
    // only the latest thirty are kept
    let mut events = vec![win_ev("five_hour", 0.0, 100, ""), win_ev("five_hour", 0.01, 100, "")];
    for i in 0..40 {
        events.extend(pair(10.0 + f64::from(i), 0.01 + 0.01 * f64::from(i)));
    }
    assert_eq!(said(&feed(events), FIVE), (30.0, 34.5), "pairs 10 to 49: the latest thirty are 20 to 49");
}

#[test]
fn a_pair_is_of_one_doer_one_account_and_one_window() {
    let (a, b) = (|u: f64| win_ev("five_hour", u, 100, "work"), |u: f64| win_ev("five_hour", u, 100, ""));
    // the work account's cost is not the other account's
    let f = feed(vec![a(0.0), b(0.0), a(0.01), b(0.01), spent(0.14, "work"), spent(0.28, ""), a(0.02), b(0.02)]);
    assert_eq!((said(&f, "claude|work|five_hour"), said(&f, FIVE)), ((1.0, 14.0), (1.0, 28.0)));
    // the week takes the cost of the same request as the five hours do
    let week = |u: f64| win_ev("seven_day", u, 900, "");
    let f = feed(vec![b(0.0), week(0.0), b(0.01), week(0.01), spent(0.14, ""), b(0.02), week(0.02)]);
    assert_eq!((said(&f, FIVE), said(&f, "claude||seven_day")), ((1.0, 14.0), (1.0, 14.0)));
    // what no doer wrote, and what is not a reading, is not read
    let other = ev("run.window", Actor::Core, json!({ "window": "five_hour", "utilization": 0.5, "resets_at": 1 }));
    let junk = ev("run.window", Actor::Doer("claude".into()), json!({ "window": "five_hour" }));
    assert!(feed(vec![other, junk]).read(&[], 0) == Value::Map(Default::default()));
}

#[test]
fn what_the_fold_has_learned_survives_a_save_and_a_reset_forgets_it() {
    let at = |u: f64| win_ev("five_hour", u, 100, "");
    let mut f = feed(vec![at(0.0), at(0.01), spent(0.14, ""), at(0.02), spent(0.07, "")]);
    let mut g = PlanRate::default();
    g.load(f.save()).unwrap();
    assert_eq!(said(&g, FIVE), (1.0, 14.0));
    // the anchor and the cost since it are kept too: the next move is a pair of what was spent before the restart and after it
    g.apply(&spent(0.07, ""));
    g.apply(&at(0.03));
    assert_eq!(said(&g, FIVE), (2.0, 14.0));
    assert!(g.load(json!("not a state")).is_err());
    f.reset();
    assert_eq!(f.read(&[], 0), Value::Map(Default::default()));
    assert!(f.wants("run.window") && f.wants("run.metrics") && !f.wants("run.shell"));
}

fn plan_with(mine: &str) -> Plan {
    plan(mine)
}

/// A plan with a price and no `assume`.
fn other() -> Plan {
    Plans::load(&base_tables(NO_ASSUME)).0.of("other", None).expect("the row is a plan").clone()
}

#[test]
fn the_plan_cost_of_a_request_is_measured_by_the_windows_that_have_pairs_and_assumed_otherwise() {
    let p = plan_with(PRICE);
    let (five, week) = (24.0 * (5.0 / 24.0) / 30.44, 24.0 * 7.0 / 30.44);
    let rates = |five_n: usize, week_n: usize| move |w: &str| Some(if w == "five_hour" { (five_n, 14.0) } else { (week_n, 300.0) });
    // both measured: the larger share binds, and the week is the larger here
    let c = p.cost(1.0, 5, rates(5, 5)).unwrap();
    assert_eq!((c.window.as_deref(), c.source.as_str()), (Some("seven_day"), "measured n=5"));
    assert!((c.usd - week / 300.0).abs() < 1e-9 && week / 300.0 > five / 14.0);
    // only the five hours measured: they are what is charged, and the week's assumption does not outvote them
    let c = p.cost(1.0, 5, rates(7, 4)).unwrap();
    assert_eq!((c.window.as_deref(), c.source.as_str()), (Some("five_hour"), "measured n=7"));
    assert!((c.usd - five / 14.0).abs() < 1e-9);
    // none measured, or too few: the assumption, with no window
    let c = p.cost(0.5, 5, rates(4, 4)).unwrap();
    assert_eq!((c.window, c.source.as_str()), (None, "assumed"));
    assert!((c.usd - 0.5 * 24.0 / 1000.0).abs() < 1e-9);
    // a median of nothing is no measure, and `min_n` of 0 asks for one pair
    assert_eq!(p.cost(1.0, 0, |_| Some((1, 0.0))).unwrap().source, "assumed");
    assert_eq!(p.cost(1.0, 0, |_| Some((0, 14.0))).unwrap().source, "assumed");
    assert_eq!(p.cost(1.0, 0, |_| Some((1, 14.0))).unwrap().source, "measured n=1");
    // a measure above the assumption is the measure: the assumption was wrong, and the plan cost says so
    assert!(p.cost(1.0, 1, |_| Some((1, 2.0))).unwrap().usd > 24.0 / 1000.0);
    // no price: no plan cost; a price and nothing to go by: none
    assert_eq!(plan("").cost(1.0, 1, |_| Some((9, 14.0))), None);
    assert_eq!(other().cost(1.0, 5, |_| None), None);
    assert_eq!(other().cost(1.0, 5, |_| Some((5, 14.0))).unwrap().source, "measured n=5");
}

/// The operator's `cost.plan.min_n`, one pair is enough.
const ONE_PAIR: &str = "[[setting]]\nid = \"cost.plan.min_n\"\ndoc = \"x\"\nty = \"int\"\ndefault = 5\nvalue = 1\n";

fn cases(mine: &str, doer: &str, beats: Vec<Beat>) -> (Rig, Vec<Json>, Vec<Json>) {
    let plans = Plans::load(&base_tables(mine)).0;
    let rig = Rig::based(vec![Scripted::new(doer, vec![beats])], mine, |c| {
        c.plans = Arc::new(plans);
        assert!(c.folds.add(Box::<PlanRate>::default(), c.log.head()).is_empty());
    });
    let id = rig.spawn("go", doer);
    rig.finished(&id);
    let of = |kind: &str| rig.of_kind(kind).into_iter().map(|e| e.data).collect::<Vec<_>>();
    let (metrics, windows) = (of("run.metrics"), of("run.window"));
    (rig, metrics, windows)
}

fn limits(week: f64, five: f64) -> Beat {
    Beat::Limits(vec![("five_hour", five, 1_000), ("seven_day", week, 9_000)])
}

#[test]
fn the_windows_reach_the_log_and_a_request_on_a_plan_without_a_price_says_so() {
    let (_rig, metrics, windows) = cases("", "claude", vec![limits(0.0, 0.02), Beat::Spend(0.25), limits(0.01, 0.03), Beat::Spend(0.25), end("x")]);
    assert_eq!(windows.len(), 4, "both windows, twice: {windows:?}");
    assert_eq!(windows[0], json!({ "window": "five_hour", "utilization": 0.02, "resets_at": 1000, "status": "allowed" }));
    assert_eq!(windows[1]["window"], "seven_day");
    assert_eq!(metrics.len(), 2);
    for m in &metrics {
        assert_eq!(m["usd"], json!(0.25), "the list cost stays what it was: {m}");
        assert!(m.get("plan_usd").is_none(), "{m}");
        // no price is said in words that tell where to set it, not as a bare `not set`: the row of Library › Plans, not a file
        let words = m["plan"].as_str().unwrap();
        assert!(words.contains("no price") && words.contains("Library › Plans") && words.contains("on claude") && !words.contains("plan.toml"), "{words}");
        // and the share of the window needs no price: the assumed capacity of the plan says it, of the shortest window
        assert_eq!((m["share_of"].as_str(), m["source"].as_str()), (Some("5h"), Some("assumed")), "{m}");
        assert!((m["share"].as_f64().unwrap() - 0.25 / (1000.0 * 5.0 / 24.0 / 30.44)).abs() < 1e-9, "{m}");
    }
}

#[test]
fn a_request_on_a_priced_plan_says_its_share_of_the_window_beside_the_plan_cost() {
    let (_rig, metrics, _) = cases(PRICE, "claude", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    let m = &metrics[0];
    assert_eq!((m["share_of"].as_str(), m["source"].as_str()), (Some("5h"), Some("assumed")), "{m}");
    assert!((m["share"].as_f64().unwrap() - 0.25 / (1000.0 * 5.0 / 24.0 / 30.44)).abs() < 1e-9, "{m}");
    assert!(m["plan_usd"].as_f64().unwrap() > 0.0 && m.get("plan").is_none(), "{m}");
    // say the five-hour window's pair is $14 a full window; $0.28 is a fiftieth of it
    let beats = vec![limits(0.0, 0.00), Beat::Spend(0.07), limits(0.0, 0.01), Beat::Spend(0.14), limits(0.0, 0.02), Beat::Spend(0.28), end("x")];
    let (_rig, metrics, _) = cases(&format!("{PRICE}{ONE_PAIR}"), "claude", beats);
    let m = &metrics[2];
    assert!((m["share"].as_f64().unwrap() - 0.02).abs() < 1e-9 && m["share_of"] == "5h" && m["source"] == "measured n=1", "{m}");
}

#[test]
fn the_share_of_a_window_is_the_one_that_binds_and_needs_no_price() {
    let rates = |w: &str| Some(if w == "five_hour" { (5, 14.0) } else { (5, 300.0) });
    // priced: the week binds by money (as the plan cost has it), so it is the window named
    let s = plan_with(PRICE).share(1.0, 5, rates).unwrap();
    assert_eq!((s.of.as_str(), s.window.as_deref(), s.source.as_str()), ("7d", Some("seven_day"), "measured n=5"));
    assert!((s.frac - 1.0 / 300.0).abs() < 1e-12);
    // no price: the larger part of a window binds, the five hours
    let s = plan("").share(1.0, 5, rates).unwrap();
    assert_eq!((s.of.as_str(), s.window.as_deref()), ("5h", Some("five_hour")));
    assert!((s.frac - 1.0 / 14.0).abs() < 1e-12);
    // nothing measured, nothing assumed: no share; nothing measured: the assumed one of the shortest window
    assert_eq!(other().share(1.0, 5, |_| None), None);
    let s = plan_with(PRICE).share(0.5, 5, |_| None).unwrap();
    assert_eq!((s.of.as_str(), s.window, s.source.as_str()), ("5h", None, "assumed"));
}

#[test]
fn the_cache_parts_of_a_request_reach_the_log_beside_the_tokens() {
    let (_rig, metrics, _) = cases("", "claude", vec![Beat::Cached(0.333, 100_002, 99_000, 1_000), Beat::Spend(0.25), end("x")]);
    assert_eq!((metrics[0]["tokens_in"].as_u64(), metrics[0]["cached"].as_u64(), metrics[0]["wrote"].as_u64()), (Some(100_002), Some(99_000), Some(1_000)), "{}", metrics[0]);
    assert!(metrics[1].get("cached").is_none() && metrics[1].get("wrote").is_none(), "a doer that says none leaves the Log as it was: {}", metrics[1]);
}

#[test]
fn before_a_window_is_measured_the_plan_cost_is_the_assumed_one_and_says_so() {
    let (_rig, metrics, _) = cases(PRICE, "claude", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    let m = &metrics[0];
    // $24 over a month the plan assumes is worth $1000 of list cost: 2.4% of the request
    assert!((m["plan_usd"].as_f64().unwrap() - 0.25 * 24.0 / 1000.0).abs() < 1e-9, "{m}");
    assert_eq!((&m["source"], &m["usd"]), (&json!("assumed"), &json!(0.25)));
    assert!(m.get("window").is_none() && m.get("plan").is_none(), "{m}");
    // with nothing assumed there is nothing to say yet
    let (_rig, metrics, _) = cases(NO_ASSUME, "other", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    assert_eq!(metrics[0]["plan"], "not measured", "{}", metrics[0]);
    assert!(metrics[0].get("plan_usd").is_none());
}

#[test]
fn once_a_window_has_its_pairs_the_plan_cost_is_measured_and_names_the_window() {
    // the five-hour window moves a step twice while $0.07 and then $0.14 are spent: say a pair of $14 for a full window; the week stays put
    let beats = vec![limits(0.0, 0.00), Beat::Spend(0.07), limits(0.0, 0.01), Beat::Spend(0.14), limits(0.0, 0.02), Beat::Spend(0.28), end("x")];
    let (_rig, metrics, _) = cases(&format!("{PRICE}{ONE_PAIR}"), "claude", beats);
    assert_eq!(metrics[0]["source"], "assumed", "no pair yet: {}", metrics[0]);
    assert_eq!(metrics[1]["source"], "assumed", "a first move only says where a step begins: {}", metrics[1]);
    let m = &metrics[2];
    let five = 24.0 * (5.0 / 24.0) / 30.44;
    assert!((m["plan_usd"].as_f64().unwrap() - 0.28 * five / 14.0).abs() < 1e-9, "{m}");
    assert_eq!((&m["source"], &m["window"], &m["usd"]), (&json!("measured n=1"), &json!("five_hour"), &json!(0.28)));
    // the same pair is too few while `cost.plan.min_n` is its default of five
    let beats = vec![limits(0.0, 0.00), Beat::Spend(0.07), limits(0.0, 0.01), Beat::Spend(0.14), limits(0.0, 0.02), Beat::Spend(0.28), end("x")];
    let (_rig, metrics, _) = cases(PRICE, "claude", beats);
    assert_eq!(metrics[2]["source"], "assumed", "{}", metrics[2]);
}

#[test]
fn the_plan_a_task_is_paid_by_is_the_one_of_the_account_it_runs_as() {
    let work = "[[plan]]\nid = \"work\"\nconnector = \"claude\"\naccount = \"work\"\nwindows = [\"seven_day=7d\"]\nprice = \"$100\"\nassume = \"$2000\"\n";
    let turn = vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")];
    let plans = Plans::load(&base_tables(work)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![turn.clone(), turn])], work, |c| c.plans = Arc::new(plans));
    let mut spec = TaskSpec::new("go", "claude");
    spec.account = Some("work".into());
    let (mine, other) = (rig.tasks.spawn(spec, Actor::Operator).expect("spawns"), rig.spawn("go", "claude"));
    rig.finished(&mine);
    rig.finished(&other);
    let of = |id: &str, kind: &str| rig.events().into_iter().find(|e| e.kind == kind && e.task.as_deref() == Some(id)).map(|e| e.data);
    let m = of(&mine, "run.metrics").unwrap();
    assert!((m["plan_usd"].as_f64().unwrap() - 0.25 * 100.0 / 2000.0).abs() < 1e-9, "the account's plan: {m}");
    assert_eq!((&m["account"], &of(&mine, "run.window").unwrap()["account"]), (&json!("work"), &json!("work")), "the Log says whose windows these are");
    let o = of(&other, "run.metrics").unwrap();
    assert!(o["plan"].as_str().unwrap().contains("no price"), "the plan for any has no price: {o}");
    assert!(o.get("account").is_none());
}

#[test]
fn the_pairs_of_an_account_price_its_own_requests() {
    let work = "[[plan]]\nid = \"work\"\nconnector = \"claude\"\naccount = \"work\"\nwindows = [\"five_hour=5h\"]\nprice = \"$100\"\nassume = \"$2000\"\n";
    let beats = vec![limits(0.0, 0.00), Beat::Spend(0.07), limits(0.0, 0.01), Beat::Spend(0.14), limits(0.0, 0.02), Beat::Spend(0.28), end("x")];
    let mine = format!("{work}{ONE_PAIR}");
    let plans = Plans::load(&base_tables(&mine)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![beats])], &mine, |c| {
        c.plans = Arc::new(plans);
        assert!(c.folds.add(Box::<PlanRate>::default(), c.log.head()).is_empty());
    });
    let mut spec = TaskSpec::new("go", "claude");
    spec.account = Some("work".into());
    rig.finished(&rig.tasks.spawn(spec, Actor::Operator).expect("spawns"));
    let last = rig.of_kind("run.metrics").pop().unwrap().data;
    assert_eq!((&last["source"], &last["window"], &last["account"]), (&json!("measured n=1"), &json!("five_hour"), &json!("work")), "{last}");
    assert!((last["plan_usd"].as_f64().unwrap() - 0.28 * (100.0 * 5.0 / 24.0 / 30.44) / 14.0).abs() < 1e-9, "{last}");
}

#[test]
fn a_request_on_a_key_keeps_the_log_as_it_was_and_still_keeps_the_windows() {
    let (_rig, metrics, windows) = cases(PRICE, "alpha", vec![limits(0.0, 0.0), Beat::Spend(0.25), limits(0.02, 0.0), Beat::Spend(0.25), end("x")]);
    assert_eq!(metrics, vec![json!({ "usd": 0.25, "tokens_in": 10, "tokens_out": 5 }); 2]);
    assert_eq!(windows.len(), 4);
}

#[test]
fn the_spend_fold_of_the_base_pack_adds_up_what_a_task_cost() {
    let (folds, problems) = declared(&base_tables(""), Scope::Project);
    assert!(problems.is_empty(), "{problems:?}");
    assert!(folds.iter().any(|f| f.id() == "spend"), "the base pack declares `spend`");
    let rig = Rig::tuned(vec![Scripted::new("alpha", vec![vec![Beat::Spend(0.25), Beat::Spend(0.5), end("x")]])], |c| load_base(c, ""));
    let id = rig.spawn("go", "alpha");
    rig.finished(&id);
    let got = rig.folds.read("spend", &[Key::Name(id.clone())], rig.log.clock().now_ms());
    assert_eq!(got, Value::usd(0.75), "the cost of {id} this week");
}

#[test]
fn the_spend_model_fold_adds_up_what_each_model_cost_by_the_model_that_served() {
    let beats = vec![Beat::Served("m-5-5-full", "anthropic"), Beat::Spend(0.25), Beat::Served("other", "h"), Beat::Spend(0.5), end("x")];
    let rig = Rig::tuned(vec![Scripted::new("alpha", vec![beats])], |c| load_base(c, ""));
    rig.finished(&rig.spawn("go", "alpha"));
    let now = rig.log.clock().now_ms();
    assert_eq!(rig.folds.read("spend_model", &[Key::Name("m-5-5-full".into())], now), Value::usd(0.25));
    assert_eq!(rig.folds.read("spend_model", &[Key::Name("other".into())], now), Value::usd(0.5));
}

#[test]
fn the_served_folds_count_every_model_a_run_was_ever_served_by_and_when_last() {
    let beats = vec![Beat::Served("m-5-5-full", "anthropic"), Beat::Served("m-5-5-full", "anthropic"), Beat::Served("other", "h"), end("x")];
    let rig = Rig::tuned(vec![Scripted::new("alpha", vec![beats])], |c| load_base(c, ""));
    rig.finished(&rig.spawn("go", "alpha"));
    let now = rig.log.clock().now_ms();
    assert_eq!(rig.folds.read("models_served", &[Key::Name("m-5-5-full".into())], now), Value::num(2.0));
    assert_eq!(rig.folds.read("models_served", &[Key::Name("other".into())], now), Value::num(1.0));
    let last = rig.folds.read("models_last", &[Key::Name("m-5-5-full".into())], now);
    assert!(matches!(last, Value::Num(at) if at > 0.0), "the event's own time: {last:?}");
}

// ---- the switch, the sources, the caps and the size of a calibration ----

fn setting(id: &str, ty: &str, value: &str) -> String {
    format!("[[setting]]\nid = \"{id}\"\ndoc = \"x\"\nty = \"{ty}\"\ndefault = {value}\nvalue = {value}\n")
}

#[test]
fn the_sources_say_where_a_connector_shows_its_windows_and_a_connector_with_none_is_priced_by_its_list_cost() {
    let (plans, problems) = Plans::load(&base_tables(""));
    assert!(problems.is_empty(), "{problems:?}");
    let path = |c: &str| plans.source(c).map(|s| s.windows.clone());
    assert_eq!(path("claude"), Some(Some("rate_limit_info.unifiedWindows".into())));
    assert_eq!((path("pi"), path("omp")), (Some(None), Some(None)), "they show a cost and no window");
    assert_eq!(path("test"), None, "a connector with no row is asked nothing");
    // a plan on a connector that shows none: the request has the list cost alone
    let pi = "[[plan]]\nid = \"pi\"\nconnector = \"pi\"\nwindows = [\"seven_day=7d\"]\nprice = \"$24\"\nassume = \"$1000\"\n";
    let (_rig, metrics, windows) = cases(pi, "pi", vec![Beat::Spend(0.25), end("x")]);
    assert_eq!(metrics, vec![json!({ "usd": 0.25, "tokens_in": 10, "tokens_out": 5 })]);
    assert!(windows.is_empty());
}

#[test]
fn a_stream_that_shows_no_window_when_it_should_says_so_once_and_the_request_is_priced_without() {
    let turn = vec![Beat::Spend(0.25), end("x")];
    let plans = Plans::load(&base_tables(PRICE)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![turn.clone(), turn.clone(), vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]])], PRICE, |c| c.plans = Arc::new(plans));
    for _ in 0..3 {
        let id = rig.spawn("go", "claude");
        rig.finished(&id);
    }
    let said = rig.of_kind("cost.path_missing");
    assert_eq!(said.len(), 1, "once for the connector, not once a run: {said:?}");
    assert_eq!(said[0].data, json!({ "doer": "claude", "path": "rate_limit_info.unifiedWindows" }));
    let metrics = rig.of_kind("run.metrics");
    assert_eq!(metrics.len(), 3);
    assert!(metrics.iter().all(|m| m.data["source"] == "assumed"), "priced all the same, by the assumption: {metrics:?}");
    // a run that showed its windows says nothing
    let (rig, ..) = cases(PRICE, "claude", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    assert!(rig.of_kind("cost.path_missing").is_empty());
}

#[test]
fn with_the_switch_off_nothing_reads_a_window_and_the_log_has_the_list_cost_alone() {
    let off = format!("{PRICE}{}", setting("cost.plan.on", "bool", "false"));
    let (rig, metrics, windows) = cases(&off, "claude", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    assert!(windows.is_empty(), "no window is read: {windows:?}");
    assert_eq!(metrics, vec![json!({ "usd": 0.25, "tokens_in": 10, "tokens_out": 5 })]);
    assert!(rig.of_kind("cost.path_missing").is_empty(), "and none is missed");
    assert!(rig.tasks.calibration("claude", None).unwrap_err().contains("cost.plan.on"));
    // on, which is how it stands, the same turn is read
    let (_rig, metrics, windows) = cases(PRICE, "claude", vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]);
    assert_eq!((windows.len(), metrics[0]["source"].as_str()), (2, Some("assumed")));
}

fn capped(mine: &str, soft: f64, spend: f64) -> Vec<Json> {
    let plans = Plans::load(&base_tables(mine)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![vec![limits(0.0, 0.0), Beat::Spend(spend), end("x")]])], mine, |c| c.plans = Arc::new(plans));
    let mut spec = TaskSpec::new("go", "claude");
    spec.limits.usd_soft = Some(soft);
    let id = rig.tasks.spawn(spec, Actor::Operator).expect("spawns");
    rig.finished(&id);
    rig.of_kind("task.cap_reached").into_iter().map(|e| e.data).collect()
}

#[test]
fn a_cap_on_a_plan_is_in_what_the_task_took_of_the_plan_and_in_list_cost_with_the_switch_off() {
    // $0.25 of list cost is $0.006 of a plan the assumption says is worth $1000 of $24
    assert!(capped(PRICE, 0.01, 0.25).is_empty(), "a cap of a cent is not reached by 0.6 cents of the plan");
    let reached = capped(PRICE, 0.004, 0.25);
    assert_eq!(reached.len(), 1, "{reached:?}");
    assert!((reached[0]["spent"].as_f64().unwrap() - 0.006).abs() < 1e-9 && reached[0]["soft"] == json!(0.004), "{reached:?}");
    // off, or on a plan with no price, the list cost is what is capped
    assert_eq!(capped(&format!("{PRICE}{}", setting("cost.plan.on", "bool", "false")), 0.01, 0.25).len(), 1);
    assert_eq!(capped("", 0.01, 0.25).len(), 1, "no price, no plan cost");
}

#[test]
fn a_task_past_its_hard_cap_in_plan_cost_is_stopped_and_not_before() {
    let plans = Plans::load(&base_tables(PRICE)).0;
    let turn = vec![limits(0.0, 0.0), Beat::Spend(0.25), Beat::Spend(0.25), end("x")];
    let rig = Rig::based(vec![Scripted::new("claude", vec![turn.clone(), turn])], PRICE, |c| c.plans = Arc::new(plans));
    // $0.012 of the plan after two turns: past 1.5 × $0.007, short of 1.5 × $0.009
    for (soft, stops) in [(0.007, true), (0.009, false)] {
        let mut spec = TaskSpec::new("go", "claude");
        spec.limits.usd_soft = Some(soft);
        let id = rig.tasks.spawn(spec, Actor::Operator).expect("spawns");
        rig.finished(&id);
        let row = rig.tasks.book().get(&id).unwrap();
        assert_eq!(matches!(row.state, j4n_core::task::State::Stopped { why: j4n_core::task::Why::HardCap }), stops, "{soft}: {:?}", row.state);
    }
}

#[test]
fn a_calibration_is_sized_for_the_shortest_window_from_what_a_full_one_is_worth() {
    let p = plan(PRICE);
    let none = |_: &str| None;
    // nothing measured: the assumption, $1000 over a month, for the five hours; two requests while the window has never moved
    let c = p.calibration(none, |_| false).unwrap();
    let per = 1000.0 * 5.0 / 24.0 / 30.44;
    assert_eq!((c.window.as_str(), c.source.as_str(), c.requests), ("five_hour", "assumed", 2));
    assert!((c.per - per).abs() < 1e-9 && (c.target - per * 0.02).abs() < 1e-9 && c.share == 0.02, "{c:?}");
    assert!((c.money.unwrap() - 24.0 * 5.0 / 24.0 / 30.44 * 0.02).abs() < 1e-9, "{c:?}");
    // a window that has moved needs the one request; a measured window is what the median says
    let c = p.calibration(|w| (w == "five_hour").then_some((8, 14.0)), |_| true).unwrap();
    assert_eq!((c.source.as_str(), c.requests, c.per), ("measured n=8", 1, 14.0));
    assert!((c.target - 0.28).abs() < 1e-9);
    // a median of nothing is not a measure
    assert_eq!(p.calibration(|_| Some((3, 0.0)), |_| true).unwrap().source, "assumed");
    // no price: the size is the same, and the plan has no money; nothing to go by: nothing to size
    assert_eq!(plan("").calibration(none, |_| true).unwrap().money, None);
    assert_eq!(other().calibration(none, |_| true), None);
}

#[test]
fn the_loop_sizes_a_calibration_for_the_plan_of_a_doer_and_knows_when_its_window_has_moved() {
    let turn = vec![limits(0.0, 0.00), Beat::Spend(0.07), limits(0.0, 0.01), end("x")];
    let plans = Plans::load(&base_tables(PRICE)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![turn])], PRICE, |c| {
        c.plans = Arc::new(plans);
        assert!(c.folds.add(Box::<PlanRate>::default(), c.log.head()).is_empty());
    });
    let c = rig.tasks.calibration("claude", None).unwrap();
    assert_eq!((c.requests, c.size, c.source.as_str()), (2, 3000, "assumed"), "{c:?}");
    let id = rig.spawn("go", "claude");
    rig.finished(&id);
    assert_eq!(rig.tasks.calibration("claude", None).unwrap().requests, 1, "the five hours moved: the next request makes a pair");
    assert_eq!(rig.tasks.calibration("claude", Some("work")).unwrap().requests, 2, "another account's window has not");
    // a window read once, and not moved since, is somewhere inside a step: two requests still
    let read = vec![limits(0.0, 0.03), end("x")];
    let plans = Plans::load(&base_tables(PRICE)).0;
    let rig = Rig::based(vec![Scripted::new("claude", vec![read])], PRICE, |c| {
        c.plans = Arc::new(plans);
        assert!(c.folds.add(Box::<PlanRate>::default(), c.log.head()).is_empty());
    });
    let id = rig.spawn("go", "claude");
    rig.finished(&id);
    assert_eq!(rig.tasks.calibration("claude", None).unwrap().requests, 2);
    assert!(rig.tasks.calibration("zeta", None).unwrap_err().contains("no plan"));
    assert!(rig.tasks.calibration("pi", None).unwrap_err().contains("no window"));
}

// ---- what is shown, and what the election weighs ----

#[test]
fn the_plan_cost_is_said_first_with_the_list_cost_beside_it_and_a_request_on_no_plan_has_one_number() {
    use j4n_core::cost::Show;
    let m = |show: Show, planned, cap| show.money(planned, "$0.0060", cap, "$0.2500");
    assert_eq!(m(Show::Both, true, None), "$0.0060 plan · $0.2500 list");
    assert_eq!(m(Show::Plan, true, None), "$0.0060 plan");
    assert_eq!(m(Show::List, true, None), "$0.2500 list");
    // a cap is in the plan cost, so it is said with it, whatever else is shown
    assert_eq!(m(Show::Both, true, Some("$1.0000")), "$0.0060 of $1.0000 plan · $0.2500 list");
    assert_eq!(m(Show::List, true, Some("$1.0000")), "$0.0060 of $1.0000 plan · $0.2500 list");
    assert_eq!(m(Show::Plan, true, Some("$1.0000")), "$0.0060 of $1.0000 plan");
    // no plan: the one number, as it always was
    for show in [Show::Plan, Show::List, Show::Both] {
        assert_eq!(m(show, false, None), "$0.0060", "{show:?}");
        assert_eq!(m(show, false, Some("$1.0000")), "$0.0060 of $1.0000", "{show:?}");
    }
    assert_eq!([Show::word("plan"), Show::word("list"), Show::word("both"), Show::word("nonsense")], [Show::Plan, Show::List, Show::Both, Show::Both]);
}

/// A task of `doer` that spent $0.25 at list price under `mine`; the rig, the task's id and its row.
fn priced(mine: &str, doer: &str) -> (Rig, String, j4n_core::task::Row) {
    let plans = Plans::load(&base_tables(mine)).0;
    let rig = Rig::based(vec![Scripted::new(doer, vec![vec![limits(0.0, 0.0), Beat::Spend(0.25), end("x")]])], mine, |c| {
        c.plans = Arc::new(plans);
        assert!(c.folds.add(Box::<PlanRate>::default(), c.log.head()).is_empty());
    });
    let id = rig.spawn("go", doer);
    let row = rig.finished(&id);
    (rig, id, row)
}

#[test]
fn a_task_keeps_what_it_cost_at_list_price_beside_what_it_took_of_the_plan() {
    let (_rig, _, on_plan) = priced(PRICE, "claude");
    assert_eq!((on_plan.spent, on_plan.listed, on_plan.planned), (6_000, 250_000, true), "$0.25 at list is 2.4 % of it of a plan");
    let (_rig, _, on_key) = priced("", "claude");
    assert_eq!((on_key.spent, on_key.listed, on_key.planned), (250_000, 250_000, false), "no price: one number");
    let (_rig, _, off) = priced(&format!("{PRICE}{}", setting("cost.plan.on", "bool", "false")), "claude");
    assert_eq!((off.spent, off.listed, off.planned), (250_000, 250_000, false), "the switch off: one number");
}

fn spend_page(rig: &Rig, show: Option<&str>) -> Vec<Value> {
    let mut screen = j4n_core::front::screen::Screen::builtin();
    screen.folds = Some(rig.folds.clone());
    if let Some(w) = show {
        screen.set = j4n_core::setting::Settings::from_pairs([("cost.show", Value::text(w))]).tree();
    }
    screen.spend_rows(rig.log.clock().now_ms())
}

fn money_of(rows: &[Value], id: &str) -> Option<String> {
    rows.iter().find_map(|r| match r {
        Value::Map(m) if m.get("id") == Some(&Value::text(id)) => match m.get("money") {
            Some(Value::Str(s)) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    })
}

#[test]
fn the_spend_page_says_what_a_task_took_of_the_plan_with_its_list_cost_and_as_cost_show_says() {
    let (rig, id, _) = priced(PRICE, "claude");
    let (both, plan, list) = (spend_page(&rig, None), spend_page(&rig, Some("plan")), spend_page(&rig, Some("list")));
    assert_eq!(money_of(&both, &id).as_deref(), Some("$0.0060 plan · $0.2500 list"), "{both:?}");
    assert_eq!(money_of(&plan, &id).as_deref(), Some("$0.0060 plan"));
    assert_eq!(money_of(&list, &id).as_deref(), Some("$0.2500 list"));
    // a task on no plan has the one number, whatever is asked
    let (rig, id, _) = priced("", "claude");
    for show in [None, Some("plan"), Some("list")] {
        assert_eq!(money_of(&spend_page(&rig, show), &id).as_deref(), Some("$0.2500"), "{show:?}");
    }
}

#[test]
fn the_election_prices_a_doer_on_a_plan_as_a_task_is_and_a_doer_without_one_at_list_price() {
    use j4n_core::front::hosts::PlanPrice;
    let (rig, ..) = priced(PRICE, "claude");
    let pricing = |mine: &str, on: &str| {
        let tables = base_tables(&format!("{mine}{on}"));
        j4n_core::cost::PlanPricing { plans: j4n_core::cost::LivePlans::of(Arc::new(Plans::load(&tables).0)), folds: rig.folds.clone(), settings: j4n_core::setting::Settings::load(&tables).0.into() }
    };
    // nothing measured: the assumption, $24 of $1000, as a task's request is
    let got = pricing(PRICE, "").price("claude", 0.5, 0).unwrap();
    assert!((got - 0.5 * 24.0 / 1000.0).abs() < 1e-9, "{got}");
    assert_eq!(pricing(PRICE, "").price("other", 0.5, 0), None, "a doer with no plan");
    assert_eq!(pricing("", "").price("claude", 0.5, 0), None, "a plan with no price");
    assert_eq!(pricing(PRICE, &setting("cost.plan.on", "bool", "false")).price("claude", 0.5, 0), None, "the switch off");
    // a connector that shows no window is priced by its list cost, even if a plan row names it
    let named = "[[plan]]\nid = \"pi\"\nconnector = \"pi\"\nwindows = [\"seven_day=7d\"]\nprice = \"$20\"\nassume = \"$500\"\n";
    assert_eq!(pricing(named, "").price("pi", 0.5, 0), None);
}

#[test]
fn the_election_prices_with_the_plan_for_any_account_and_cost_show_is_both_until_the_operator_says_otherwise() {
    use j4n_core::front::hosts::PlanPrice;
    let (rig, ..) = priced(PRICE, "claude");
    // a plan of an account of its own beside the plan for any: the election has no account, so it is the plan for any
    let work = "[[plan]]\nid = \"work\"\nconnector = \"claude\"\naccount = \"work\"\nwindows = [\"seven_day=7d\"]\nprice = \"$100\"\nassume = \"$2000\"\n";
    let tables = base_tables(&format!("{PRICE}{work}"));
    let pricing = j4n_core::cost::PlanPricing { plans: j4n_core::cost::LivePlans::of(Arc::new(Plans::load(&tables).0)), folds: rig.folds.clone(), settings: j4n_core::setting::Settings::load(&tables).0.into() };
    assert!((pricing.price("claude", 1.0, 0).unwrap() - 24.0 / 1000.0).abs() < 1e-9);
    assert_eq!(j4n_core::setting::Settings::load(&base_tables("")).0.word("cost.show", "unset"), "both");
}

#[test]
fn the_spend_page_draws_the_money_cell_of_its_rows() {
    struct One;
    impl j4n_core::view::Sources for One {
        fn rows(&self, _: &j4n_core::query::Coll) -> Vec<Value> {
            vec![Value::Map(std::collections::BTreeMap::from([("id".to_string(), Value::text("t1")), ("value".into(), Value::Usd(250_000)), ("money".into(), Value::text("$0.0060 plan · $0.2500 list"))]))]
        }
    }
    let screen = j4n_core::front::screen::Screen::builtin();
    let rows = screen.views.get("spend").expect("the spend view").rows(&One, &j4n_core::view::outer(screen.set.clone()));
    assert_eq!(rows[0].cells.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["t1", "$0.0060 plan · $0.2500 list"]);
}
