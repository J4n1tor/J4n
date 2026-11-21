//! Who does a thing the doers offered for a role, the standing numbers that rank them, and the host of the model
//! elected with the doer.

use crate::front_cases::base;
use crate::scripted::{Beat, Rig, Scripted, end};
use j4n_core::answer::chain::Registry;
use j4n_core::answer::point::points;
use j4n_core::answer::state::Answerers;
use j4n_core::doer::Doer;
use j4n_core::flow::run::Elect;
use j4n_core::front::hosts::{Hosts, NoHosts, Pick, Want};
use j4n_core::front::who::{Pin, Standing, Who};
use j4n_core::rank::{Columns, RankAnswerer, specs};
use j4n_core::task::{Asks, Mode, TaskSpec};
use j4n_core::change::Actor;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The ask points of the base pack with the rank answerer, as an election walks them.
pub fn asks_for(standing: &Standing) -> Arc<Asks> {
    let tables = base();
    let (pts, problems) = points(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let (ranks, problems) = specs(&tables);
    assert!(problems.is_empty(), "{problems:?}");
    let mut reg = Registry::new();
    reg.register(Arc::new(RankAnswerer::new(ranks, &pts, Box::new(standing.clone()))));
    Arc::new(Asks::new(pts, reg, Answerers::new()))
}

pub fn who(doers: &[Arc<Scripted>], roles: &[(&str, &[&str])], standing: &Standing, hosts: Arc<dyn Hosts>) -> (Who, Rig) {
    let asks = asks_for(standing);
    let rig = Rig::new(doers.to_vec());
    let all: Vec<Arc<dyn Doer>> = doers.iter().map(|d| d.clone() as Arc<dyn Doer>).collect();
    let roles: BTreeMap<String, Vec<String>> = roles.iter().map(|(d, r)| (d.to_string(), r.iter().map(|x| x.to_string()).collect())).collect();
    standing.set(rig.tasks.book().clone());
    (Who::new(asks, &all, roles, hosts, rig.log.clone()), rig)
}

pub fn script(id: &str, answer: &str) -> Arc<Scripted> {
    Scripted::new(id, vec![vec![end(answer)]])
}

fn ids(w: &Who, role: &str, exclude: &[&str]) -> Vec<String> {
    w.ready(role, &exclude.iter().map(|e| e.to_string()).collect::<Vec<_>>()).into_iter().map(|o| o.id).collect()
}

#[test]
fn a_doer_is_offered_for_a_role_when_it_is_ready_and_says_it_is_good_for_it_and_is_not_excluded() {
    let (a, b, c) = (script("alpha", "x"), script("beta", "x"), Scripted::probed("gamma", true, false, vec![vec![end("x")]]));
    let (w, _rig) = who(&[a, b, c], &[("alpha", &["plan", "work"]), ("beta", &["review"])], &Standing::new(), Arc::new(NoHosts));
    assert_eq!(ids(&w, "plan", &[]), ["alpha"], "beta says review only; gamma is not logged in");
    assert_eq!(ids(&w, "review", &[]), ["beta"]);
    assert_eq!(ids(&w, "work", &["alpha"]), Vec::<String>::new(), "excluded, and nobody else works");
    // a doer with no roles listed is good for any
    let (w, _rig) = who(&[script("free", "x")], &[], &Standing::new(), Arc::new(NoHosts));
    assert_eq!(ids(&w, "validate", &[]), ["free"]);
    // nobody to offer is an answer in words
    let (w, _rig) = who(&[script("alpha", "x")], &[("alpha", &["plan"])], &Standing::new(), Arc::new(NoHosts));
    let why = w.doer("validate", None, "check it", &[]).unwrap_err();
    assert!(why.contains("good for `validate`"), "{why}");
    let why = w.doer("plan", None, "plan it", &["alpha".to_string()]).unwrap_err();
    assert!(why.contains("no doer other than alpha"), "{why}");
}

#[test]
fn a_doer_whose_only_role_is_one_nothing_asks_for_is_offered_for_none() {
    let (w, _rig) = who(&[script("claude", "x"), script("test", "x")], &[("claude", &["plan", "work", "review", "validate"]), ("test", &["scripted"])], &Standing::new(), Arc::new(NoHosts));
    for role in ["plan", "work", "scout", "review", "validate", "chore"] {
        assert!(!ids(&w, role, &[]).contains(&"test".to_string()), "{role}");
    }
    assert_eq!(w.doer("work", None, "fix it", &[]).unwrap().0, "claude");
    // `[]` would not do: a doer with no roles is good for any
    let (w, _rig) = who(&[script("test", "x")], &[("test", &[])], &Standing::new(), Arc::new(NoHosts));
    assert_eq!(ids(&w, "work", &[]), ["test"]);
}

#[test]
fn a_flow_and_the_operator_are_never_elected() {
    let alpha = script("alpha", "x");
    let flowy = script("flow:plan", "x");
    let (w, _rig) = who(&[alpha, flowy], &[], &Standing::new(), Arc::new(NoHosts));
    assert_eq!(ids(&w, "work", &[]), ["alpha"]);
}

#[test]
fn what_a_doer_has_done_for_a_role_decides_between_doers_and_a_good_record_in_one_role_says_nothing_of_another() {
    let (f, g) = (Scripted::new("flaky", vec![vec![Beat::Fail("it broke".into())]]), script("steady", "ok"));
    let standing = Standing::new();
    let (w, rig) = who(&[f, g], &[], &standing, Arc::new(NoHosts));
    for doer in ["flaky", "steady"] {
        for _ in 0..3 {
            let mut spec = TaskSpec::new("go", doer);
            spec.role = Some("review".into());
            let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
            rig.finished(&id);
        }
    }
    let (fc, sc) = (standing.columns("who:review", "flaky")["reliability"], standing.columns("who:review", "steady")["reliability"]);
    assert!(sc > fc, "{sc} vs {fc}");
    assert_eq!(w.doer("review", None, "r", &[]).unwrap().0, "steady", "the better record wins over the smaller id");
    // the record is for the role: for planning there is none, and the tie goes to the smaller id again
    assert!(standing.columns("who:plan", "steady").is_empty());
    assert_eq!(w.doer("plan", None, "p", &[]).unwrap().0, "flaky");
    // a point that is not a role has no numbers
    assert!(standing.columns("intake", "steady").is_empty() && Standing::new().columns("who:plan", "steady").is_empty());
}

struct Fixed(Mutex<Vec<(String, String, bool, bool)>>);

impl Hosts for Fixed {
    fn elect(&self, doer: &str, _: Option<&str>, want: &Want, _: u64) -> Option<Pick> {
        self.0.lock().unwrap().push((doer.into(), want.role.clone(), want.waiting, want.blocked));
        Some(Pick { host: "Cerebras".into(), model: "vendor/m".into(), why: "the cheapest".into(), alts: vec![], lines: vec![], lambda: 0.003 })
    }

    fn cooling(&self, _: &str, _: Option<&str>, _: &str, _: u64) -> Option<String> {
        None
    }

    fn another(&self, _: &str, _: Option<&str>, _: &str, _: &str, _: u64) -> Option<(Pick, String)> {
        None
    }
}

#[test]
fn an_election_names_the_model_and_the_host_and_knows_whether_the_operator_is_waiting() {
    let hosts = Arc::new(Fixed(Mutex::default()));
    let (w, _rig) = who(&[script("alpha", "x")], &[], &Standing::new(), hosts.clone());
    let e = w.elect("review", "look at it", Some("main"), &[], Mode::Read).unwrap();
    assert_eq!((e.doer.as_str(), e.model.as_deref(), e.endpoint.as_deref()), ("alpha", Some("vendor/m"), Some("Cerebras")));
    assert!(e.why.contains("the cheapest"), "{}", e.why);
    // nobody has spoken: a flow's step is held up, but no one is at the keyboard
    assert_eq!(hosts.0.lock().unwrap()[0], ("alpha".into(), "review".into(), false, true));
    w.said();
    w.elect("work", "go", None, &[], Mode::Write).unwrap();
    assert_eq!(hosts.0.lock().unwrap()[1], ("alpha".into(), "work".into(), true, true), "a line was typed a moment ago");
    // without hosts the doer's own default stands
    let (w, _rig) = who(&[script("alpha", "x")], &[], &Standing::new(), Arc::new(NoHosts));
    let e = w.elect("work", "go", None, &[], Mode::Write).unwrap();
    assert_eq!((e.model, e.endpoint), (None, None));
}

#[test]
fn the_numbers_of_a_record_are_a_smoothed_rate_and_dollars_for_each_success() {
    let a = script("steady", "ok");
    let standing = Standing::new();
    let (_who, rig) = who(&[a], &[], &standing, Arc::new(NoHosts));
    for _ in 0..3 {
        let mut spec = TaskSpec::new("go", "steady");
        spec.role = Some("work".into());
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        rig.finished(&id);
    }
    let cols = standing.columns("who:work", "steady");
    // three that ended well and none that did not: (3 + 1) / (3 + 0 + 2)
    assert!((cols["reliability"] - 0.8).abs() < 1e-9, "{cols:?}");
    assert!(!cols.contains_key("cost"), "the scripted doer spends nothing");
}

#[test]
fn what_a_role_usually_needs_is_the_median_of_enough_finished_tasks_and_what_failed_counts_against_a_host() {
    use j4n_core::front::hosts::{BookRecord, Record};
    let spenders = Scripted::new("spender", vec![vec![Beat::Spend(0.5), end("ok")]]);
    let failing = Scripted::new("failer", vec![vec![Beat::Fail("it broke".into())]]);
    let rig = Rig::new(vec![spenders, failing]);
    let rec = BookRecord(rig.tasks.book().clone());
    let run = |doer: &str, role: &str, host: &str| {
        let mut spec = TaskSpec::new("go", doer);
        spec.role = Some(role.into());
        spec.tune.model = Some("m".into());
        spec.tune.endpoint = Some(host.into());
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        rig.finished(&id);
    };
    run("spender", "work", "H");
    run("spender", "work", "H");
    assert_eq!(rec.mix("work"), None, "two tasks are not enough to say");
    run("spender", "work", "H");
    assert_eq!(rec.mix("work"), Some((10, 5)), "the tokens the scripted doer reports");
    assert_eq!(rec.mix("plan"), None);
    run("failer", "work", "H");
    run("failer", "work", "G");
    assert_eq!(rec.results("m", "H", "work"), (3, 1));
    assert_eq!(rec.results("m", "G", "work"), (0, 1));
    assert_eq!(rec.results("m", "H", "plan"), (0, 0));
    assert_eq!(rec.results("other", "H", "work"), (0, 0));
}

#[test]
fn the_cost_of_a_doer_is_dollars_for_each_task_that_ended_well() {
    let spender = Scripted::new("spender", vec![vec![Beat::Spend(0.5), end("ok")]]);
    let standing = Standing::new();
    let (_who, rig) = who(&[spender], &[], &standing, Arc::new(NoHosts));
    for _ in 0..2 {
        let mut spec = TaskSpec::new("go", "spender");
        spec.role = Some("work".into());
        let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
        rig.finished(&id);
    }
    let cols = standing.columns("who:work", "spender");
    assert!((cols["cost"] - 0.5).abs() < 1e-9, "{cols:?}");
}

#[test]
fn an_election_says_which_installed_doers_it_left_out_for_want_of_a_login() {
    let (w, _rig) = who(&[script("alpha", "x"), Scripted::probed("gamma", true, false, vec![vec![end("x")]]), Scripted::probed("delta", false, true, vec![vec![end("x")]])], &[], &Standing::new(), Arc::new(NoHosts));
    let (doer, _, why) = w.doer("scout", None, "look", &[]).unwrap();
    assert_eq!(doer, "alpha");
    assert!(why.contains("left out: gamma (") && !why.contains("delta"), "{why}: the one that is installed and not logged in, with the probe's words; not the one that is not there");
    let (w, _rig) = who(&[Scripted::probed("gamma", true, false, vec![vec![end("x")]])], &[], &Standing::new(), Arc::new(NoHosts));
    let why = w.doer("scout", None, "look", &[]).unwrap_err();
    assert!(why.contains("left out: gamma ("), "{why}");
}

/// The models each doer lists, as the front's lists would say them.
pub struct Lists(pub BTreeMap<String, Vec<String>>);

impl j4n_core::front::models::ModelLists for Lists {
    fn models(&self, doer: &str) -> Option<Vec<String>> {
        self.0.get(doer).cloned()
    }
}

fn pinned(doers: &[Arc<Scripted>], roles: &[(&str, &[&str])], pin: Pin, lists: &[(&str, &[&str])]) -> (Who, Rig) {
    let (w, rig) = who(doers, roles, &Standing::new(), Arc::new(NoHosts));
    let lists = Lists(lists.iter().map(|(d, m)| (d.to_string(), m.iter().map(|x| x.to_string()).collect())).collect());
    (w.with_pin(pin, Arc::new(lists)), rig)
}

fn elect(w: &Who, role: &str, exclude: &[&str]) -> j4n_core::flow::run::Elected {
    w.elect(role, "do it", None, &exclude.iter().map(|e| e.to_string()).collect::<Vec<_>>(), Mode::Write).expect("somebody is elected")
}

#[test]
fn the_doer_of_the_launch_takes_the_role_it_is_ready_and_good_for_and_the_election_says_so() {
    let (a, b) = (script("alpha", "x"), script("beta", "x"));
    let (w, _rig) = pinned(&[a.clone(), b.clone()], &[], Pin::default(), &[]);
    assert_eq!(elect(&w, "work", &[]).doer, "alpha", "with no pin the smaller id wins the tie");
    let (w, _rig) = pinned(&[a, b], &[], Pin { doer: Some("beta".into()), model: None }, &[]);
    let e = elect(&w, "work", &[]);
    assert_eq!(e.doer, "beta", "{}", e.why);
    assert!(e.why.contains("--doer beta is pinned"), "{}", e.why);
}

#[test]
fn the_model_of_the_launch_goes_to_an_elected_doer_that_serves_it_and_one_that_does_not_keeps_its_own() {
    let (a, b) = (script("alpha", "x"), script("beta", "x"));
    let lists: &[(&str, &[&str])] = &[("alpha", &["m1", "m2"]), ("beta", &["other"])];
    let (w, _rig) = pinned(&[a.clone(), b.clone()], &[], Pin { doer: None, model: Some("m2".into()) }, lists);
    let e = elect(&w, "work", &[]);
    assert_eq!((e.doer.as_str(), e.model.as_deref()), ("alpha", Some("m2")), "{}", e.why);
    assert!(e.why.contains("--model puts it on m2"), "{}", e.why);
    // the doer that is elected does not serve it: the pick stays, and the Log's why says the pin did not fit
    let (w, _rig) = pinned(&[b], &[], Pin { doer: None, model: Some("m2".into()) }, lists);
    let e = elect(&w, "work", &[]);
    assert_eq!((e.doer.as_str(), e.model.as_deref()), ("beta", None), "{}", e.why);
    assert!(e.why.contains("--model m2 is not one of beta's models: it keeps its own"), "{}", e.why);
    // a doer that lists no models serves none; the one that was pinned by name is given the word as it was typed, as a thread is
    let (w, _rig) = pinned(&[a], &[], Pin { doer: None, model: Some("m2".into()) }, &[]);
    assert_eq!(elect(&w, "work", &[]).model, None);
    let (w, _rig) = pinned(&[script("alpha", "x")], &[], Pin { doer: Some("alpha".into()), model: Some("m2".into()) }, &[]);
    assert_eq!(elect(&w, "work", &[]).model.as_deref(), Some("m2"));
}

#[test]
fn a_pin_the_role_cannot_use_leaves_the_election_as_it_was_and_the_why_names_the_reason() {
    let (a, g) = (script("alpha", "x"), script("gamma", "x"));
    let roles: &[(&str, &[&str])] = &[("gamma", &["review"])];
    let (w, _rig) = pinned(&[a.clone(), g.clone()], roles, Pin { doer: Some("gamma".into()), model: None }, &[]);
    let e = elect(&w, "work", &[]);
    assert_eq!(e.doer, "alpha");
    assert!(e.why.contains("--doer gamma cannot take `work`: it is not good for `work`"), "{}", e.why);
    // the step leaves it out (the validator is not the one who worked)
    let (w, _rig) = pinned(&[a.clone(), script("beta", "x")], &[], Pin { doer: Some("alpha".into()), model: None }, &[]);
    let e = elect(&w, "validate", &["alpha"]);
    assert_eq!(e.doer, "beta");
    assert!(e.why.contains("--doer alpha cannot take `validate`: the step leaves it out"), "{}", e.why);
    // not signed in
    let off = Scripted::probed("gone", true, false, vec![vec![end("x")]]);
    let (w, _rig) = pinned(&[a, off], &[], Pin { doer: Some("gone".into()), model: None }, &[]);
    let e = elect(&w, "work", &[]);
    assert_eq!(e.doer, "alpha");
    assert!(e.why.contains("--doer gone cannot take `work`: it is not installed or not signed in"), "{}", e.why);
}

#[test]
fn main_is_offered_only_the_doers_that_say_main_and_the_cheaper_that_has_done_well_at_work_is_elected() {
    // the role `main` is a word of `roles`; Main's own turns are `work` turns, so a record at work is a record as Main
    let spender = |id: &str, usd: f64| Scripted::new(id, vec![vec![Beat::Spend(usd), end("ok")]]);
    let (alpha, beta, gamma) = (spender("alpha", 0.5), spender("beta", 0.1), script("gamma", "x"));
    let standing = Standing::new();
    let (w, rig) = who(&[alpha, beta, gamma], &[("alpha", &["work", "main"]), ("beta", &["work", "main"]), ("gamma", &["work"])], &standing, Arc::new(NoHosts));
    assert_eq!(ids(&w, "main", &[]), ["alpha", "beta"], "gamma does not say `main`");
    assert_eq!(ids(&w, "work", &[]), ["alpha", "beta", "gamma"]);
    for doer in ["alpha", "beta"] {
        for _ in 0..2 {
            let mut spec = TaskSpec::new("go", doer);
            spec.role = Some("work".into());
            let id = rig.tasks.spawn(spec, Actor::Operator).unwrap();
            rig.finished(&id);
        }
    }
    let (cost, work) = (standing.columns("who:main", "beta"), standing.columns("who:work", "beta"));
    assert_eq!(cost, work, "the same numbers: Main's turns are work turns");
    let (doer, _, why) = w.doer("main", None, "fix the typo", &[]).unwrap();
    assert_eq!(doer, "beta", "the cheaper of two that are equally reliable, not the smaller id: {why}");
}

/// A reading of `doer`'s limit window in the Log, as its connector's `limits` leave it.
fn window(rig: &Rig, doer: &str, status: &str, resets_in_s: i64) {
    let resets = rig.log.clock().now_ms() as i64 / 1000 + resets_in_s;
    let data = serde_json::json!({ "window": "req", "utilization": 1.0, "resets_at": resets, "status": status });
    rig.log.append(j4n_core::log::New::of("run.window", Actor::Doer(doer.into()), data)).expect("the log takes it");
}

#[test]
fn a_doer_that_says_its_own_limit_is_not_elected_while_it_is_shut_and_another_can_take_the_role() {
    let (a, b) = (script("alpha", "x"), script("beta", "x"));
    let (w, rig) = who(&[a, b], &[], &Standing::new(), Arc::new(NoHosts));
    let w = w.with_limit_readers(["alpha".to_string()].into());
    w.watch_limits(Arc::new(Mutex::new(j4n_core::front::usage::Usage::default())));
    assert_eq!(w.doer("work", None, "fix it", &[]).unwrap().0, "alpha", "nothing is shut: the standing numbers decide, as before");
    window(&rig, "alpha", "rejected", 600);
    let (doer, _, why) = w.doer("work", None, "fix it", &[]).unwrap();
    assert!(doer == "beta" && why.contains("alpha (limited, back in"), "{doer}: {why}");
    // the only doer there is takes the role, shut or not: its task waits for the limit
    let (w, rig) = who(&[script("alpha", "x")], &[], &Standing::new(), Arc::new(NoHosts));
    let w = w.with_limit_readers(["alpha".to_string()].into());
    w.watch_limits(Arc::new(Mutex::new(j4n_core::front::usage::Usage::default())));
    window(&rig, "alpha", "rejected", 600);
    assert_eq!(w.doer("work", None, "fix it", &[]).unwrap().0, "alpha");
}

/// How many probes are under way now, and the most there were at once.
type UnderWay = Arc<(Mutex<(usize, usize)>, std::sync::Condvar)>;

/// A doer whose probe is counted and, when `wait` is set, does not end until `wait` probes are under way at once (or a guard of ten seconds runs out).
struct Slow {
    id: String,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    under_way: UnderWay,
    wait: usize,
}

impl Doer for Slow {
    fn id(&self) -> &str {
        &self.id
    }

    fn probe(&self) -> j4n_core::doer::Probe {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (lock, cv) = &*self.under_way;
        let mut n = lock.lock().unwrap();
        n.0 += 1;
        n.1 = n.1.max(n.0);
        cv.notify_all();
        let guard = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while n.1 < self.wait && std::time::Instant::now() < guard {
            n = cv.wait_timeout(n, std::time::Duration::from_millis(50)).unwrap().0;
        }
        n.0 -= 1;
        j4n_core::doer::Probe { installed: true, version: Some("1".into()), authed: true, detail: String::new() }
    }

    fn caps(&self) -> j4n_core::doer::Caps {
        j4n_core::doer::Caps::default()
    }

    fn start(&self, _: j4n_core::doer::StartCx) -> Result<Box<dyn j4n_core::doer::Run>, j4n_core::table::Problem> {
        Err(j4n_core::table::Problem::refused("slow", "it only probes"))
    }
}

fn slow_who(n: usize, wait: usize) -> (Who, Rig, Arc<std::sync::atomic::AtomicUsize>, UnderWay) {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let under_way = Arc::new((Mutex::new((0, 0)), std::sync::Condvar::new()));
    let all: Vec<Arc<dyn Doer>> = (0..n).map(|i| Arc::new(Slow { id: format!("slow{i}"), calls: calls.clone(), under_way: under_way.clone(), wait }) as Arc<dyn Doer>).collect();
    let rig = Rig::new(Vec::new());
    (Who::new(asks_for(&Standing::new()), &all, BTreeMap::new(), Arc::new(NoHosts), rig.log.clone()), rig, calls, under_way)
}

#[test]
fn what_a_probe_found_stands_for_a_minute_so_the_sweeps_of_one_line_probe_a_doer_once() {
    use std::sync::atomic::Ordering::SeqCst;
    let (w, rig, calls, _) = slow_who(1, 0);
    assert_eq!(ids(&w, "work", &[]), ["slow0"]);
    assert_eq!(ids(&w, "scout", &[]), ["slow0"], "another role, the same probe");
    assert_eq!(w.doer("work", None, "fix it", &[]).unwrap().0, "slow0");
    assert_eq!(calls.load(SeqCst), 1, "three sweeps, one probe");
    rig.clock.advance(j4n_core::front::who::PROBE_FRESH_MS - 1);
    let _ = ids(&w, "work", &[]);
    assert_eq!(calls.load(SeqCst), 1, "still standing just under the minute");
    rig.clock.advance(1);
    let _ = ids(&w, "work", &[]);
    assert_eq!(calls.load(SeqCst), 2, "a minute on, the doer is probed again: a login made meanwhile is seen");
}

#[test]
fn the_probes_of_one_sweep_run_at_once_so_it_waits_for_the_slowest_and_not_for_the_sum() {
    // each probe ends only when all four are under way: probed one after another, the first would wait out its ten-second guard
    let (w, _rig, calls, under_way) = slow_who(4, 4);
    let started = std::time::Instant::now();
    assert_eq!(ids(&w, "work", &[]), ["slow0", "slow1", "slow2", "slow3"], "in the order of the doers");
    assert_eq!(under_way.0.lock().unwrap().1, 4, "all four probes were under way at once");
    assert!(started.elapsed() < std::time::Duration::from_secs(9), "no probe waited out its guard: {:?}", started.elapsed());
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
}
