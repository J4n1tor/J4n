//! Rank: the order of what is offered, with a reason; pins, exclusions, unpriced candidates, election, and the answerer.

use crate::world::*;
use j4n_core::answer::{Answerer, AskPoint, Fallback, Impact, Kind, Offer, Question, ShapeKind};
use j4n_core::ask::{Answer, AskShape, Opt};
use j4n_core::rank::{Candidate, Columns, NoColumns, RankAnswerer, Spec, rank, specs};
use j4n_core::schema::DataClass;
use j4n_core::table::{Layer, LayerKind, Tables};
use std::collections::BTreeMap;
use std::path::Path;

fn cand(id: &str, cols: &[(&str, f64)]) -> Candidate {
    Candidate { id: id.into(), cols: cols.iter().map(|(c, v)| (c.to_string(), *v)).collect(), recommended: false }
}

fn spec(weights: &[(&str, f64)]) -> Spec {
    Spec { weights: weights.iter().map(|(c, w)| (c.to_string(), *w)).collect(), ..Spec::default() }
}

fn ids(cands: &[Candidate], spec: &Spec, election: bool) -> Vec<String> {
    rank(cands, spec, election).unwrap().order.into_iter().map(|r| r.id).collect()
}

/// Three models with the numbers a standing would hold for them.
fn models() -> Vec<Candidate> {
    vec![
        cand("sturdy", &[("reliability", 0.9), ("cost", 0.50)]),
        cand("cheap", &[("reliability", 0.6), ("cost", 0.05)]),
        cand("flashy", &[("reliability", 0.7), ("cost", 3.00)]),
    ]
}

// ---- the score ----

#[test]
fn the_score_is_the_sum_of_weight_times_column_and_the_best_comes_first() {
    let r = rank(&models(), &spec(&[("reliability", 1.0)]), false).unwrap();
    assert_eq!(r.order.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["sturdy", "flashy", "cheap"]);
    assert!((r.order[0].score - 0.9).abs() < 1e-12 && (r.order[2].score - 0.6).abs() < 1e-12);
    let both = spec(&[("reliability", 1.0), ("cost", -0.1)]);
    let r = rank(&models(), &both, false).unwrap();
    assert!((r.order[0].score - (0.9 - 0.05)).abs() < 1e-12, "{:?}", r.order);
    assert_eq!(r.order.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["sturdy", "cheap", "flashy"]);
}

#[test]
fn a_weight_below_zero_makes_less_better() {
    assert_eq!(ids(&models(), &spec(&[("cost", -1.0)]), false), ["cheap", "sturdy", "flashy"]);
    assert_eq!(ids(&models(), &spec(&[("cost", 1.0)]), false), ["flashy", "sturdy", "cheap"]);
}

#[test]
fn every_place_says_why() {
    let mut s = spec(&[("reliability", 0.9)]);
    s.cost_low = true;
    let r = rank(&models(), &s, true).unwrap();
    assert!(r.order.iter().all(|x| x.why.starts_with("weights: ") && !x.why.contains('\n')), "{:?}", r.order);
    assert_eq!(r.order[0].why, "weights: cost low · reliability 0.9");
    let plain = rank(&models(), &spec(&[]), false).unwrap();
    assert!(plain.order.iter().all(|x| x.why == "no weights"));
    let mut rec = models();
    rec[1].recommended = true;
    assert_eq!(rank(&rec, &spec(&[]), false).unwrap().order[0].why, "no weights · recommended");
    let junk = rank(&models(), &spec(&[("reliability", f64::NAN), ("cost", f64::INFINITY)]), false).unwrap();
    assert!(junk.order.iter().all(|x| x.why == "no weights"), "a weight that is not a number is not shown: {:?}", junk.order);
}

#[test]
fn an_unpriced_candidate_is_never_free_it_takes_the_worst_price_known() {
    let mut c = models();
    c.push(cand("mystery", &[("reliability", 0.9)]));
    let s = spec(&[("reliability", 1.0), ("cost", -0.1)]);
    let r = rank(&c, &s, false).unwrap();
    // mystery is as reliable as sturdy but is charged what flashy costs: it is behind sturdy, and level with cheap
    let order: Vec<_> = r.order.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(order, ["sturdy", "mystery", "cheap", "flashy"]);
    let mystery = r.order.iter().find(|x| x.id == "mystery").unwrap();
    assert!((mystery.score - (0.9 - 0.3)).abs() < 1e-12, "{mystery:?}");
    assert!(mystery.why.contains("cost unknown, worst taken"), "{}", mystery.why);
    // had unpriced been read as free it would have won
    let mut free = c.clone();
    free[3].cols.insert("cost".into(), 0.0);
    assert_eq!(ids(&free, &s, false)[0], "mystery");
}

#[test]
fn a_missing_column_that_is_good_to_have_takes_the_lowest_value_known() {
    let c = vec![cand("a", &[("reliability", 0.8)]), cand("b", &[("reliability", 0.4)]), cand("c", &[])];
    let r = rank(&c, &spec(&[("reliability", 1.0)]), false).unwrap();
    let c_score = r.order.iter().find(|x| x.id == "c").unwrap().score;
    assert!((c_score - 0.4).abs() < 1e-12);
    assert_eq!(r.order[0].id, "a");
}

#[test]
fn a_column_nobody_has_counts_for_nobody() {
    let c = vec![cand("a", &[]), cand("b", &[])];
    let r = rank(&c, &spec(&[("cost", -1.0)]), false).unwrap();
    assert!(r.order.iter().all(|x| x.score == 0.0 && !x.why.contains("unknown")));
}

#[test]
fn nan_and_infinite_columns_are_ignored_like_a_missing_one() {
    let c = vec![
        cand("a", &[("reliability", 0.8), ("cost", 1.0)]),
        cand("nan", &[("reliability", 0.9), ("cost", f64::NAN)]),
        cand("inf", &[("reliability", 0.9), ("cost", f64::INFINITY)]),
        cand("ninf", &[("reliability", f64::NEG_INFINITY), ("cost", 0.0)]),
    ];
    let s = spec(&[("reliability", 1.0), ("cost", -1.0)]);
    let r = rank(&c, &s, false).unwrap();
    assert!(r.order.iter().all(|x| x.score.is_finite()), "{:?}", r.order);
    let score = |id: &str| r.order.iter().find(|x| x.id == id).unwrap().score;
    assert!((score("nan") - (0.9 - 1.0)).abs() < 1e-12, "a NaN price is the worst known, not free");
    assert!((score("inf") - (0.9 - 1.0)).abs() < 1e-12);
    assert!((score("ninf") - (0.8 - 0.0)).abs() < 1e-12, "-inf reliability is replaced by the worst finite one");
}

#[test]
fn a_weight_that_is_not_a_finite_number_is_ignored_and_the_score_stays_finite() {
    let s = spec(&[("reliability", f64::NAN), ("cost", f64::INFINITY), ("speed", 1.0)]);
    let c = vec![cand("a", &[("reliability", 1.0), ("cost", 1.0), ("speed", 2.0)])];
    assert_eq!(rank(&c, &s, false).unwrap().order[0].score, 2.0);
    let big = vec![cand("big", &[("x", f64::MAX)]), cand("neg", &[("x", -f64::MAX)])];
    let r = rank(&big, &spec(&[("x", 10.0), ("y", 1.0)]), false).unwrap();
    assert!(r.order.iter().all(|x| x.score.is_finite()), "{:?}", r.order);
    assert_eq!(r.order[0].id, "big");
    // a sum that overflows, and terms that overflow to opposite infinities, still give a finite score
    let two = vec![cand("m", &[("x", f64::MAX), ("y", f64::MAX)])];
    for weights in [[("x", 1.0), ("y", 1.0)], [("x", 10.0), ("y", -10.0)], [("x", -1.0), ("y", -1.0)]] {
        let r = rank(&two, &spec(&weights), false).unwrap();
        assert!(r.order[0].score.is_finite(), "{weights:?} -> {:?}", r.order);
    }
}

// ---- exclusions and pins ----

#[test]
fn exclude_removes_a_candidate() {
    let mut s = spec(&[("reliability", 1.0)]);
    s.exclude = vec!["sturdy".into(), "not-offered".into()];
    assert_eq!(ids(&models(), &s, false), ["flashy", "cheap"]);
}

#[test]
fn pins_hard_filter_the_offered_set_to_the_pins() {
    let mut s = spec(&[("reliability", 1.0)]);
    s.pins = vec!["cheap".into()];
    let r = rank(&models(), &s, false).unwrap();
    assert_eq!(r.order.len(), 1);
    assert_eq!(r.order[0].id, "cheap");
    assert!(r.order[0].why.starts_with("pinned · "), "{}", r.order[0].why);
    s.pins = vec!["cheap".into(), "flashy".into()];
    assert_eq!(ids(&models(), &s, false), ["flashy", "cheap"], "several pins are ranked among themselves");
}

#[test]
fn a_pin_that_is_not_offered_is_ignored_with_a_note() {
    let mut s = spec(&[("reliability", 1.0)]);
    s.pins = vec!["cheap".into(), "ghost".into()];
    let r = rank(&models(), &s, false).unwrap();
    assert_eq!(r.order.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["cheap"]);
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].contains("not offered"), "{:?}", r.notes);
    s.pins = vec!["ghost".into()];
    let r = rank(&models(), &s, false).unwrap();
    assert_eq!(r.order.len(), 3, "when no pin is offered nothing is filtered");
    assert!(r.notes[0].contains("no pin is offered"), "{:?}", r.notes);
}

#[test]
fn an_excluded_pin_stays_out_it_is_as_if_it_were_not_offered() {
    let mut s = spec(&[("reliability", 1.0)]);
    s.pins = vec!["sturdy".into()];
    s.exclude = vec!["sturdy".into()];
    assert_eq!(ids(&models(), &s, false), ["flashy", "cheap"]);
    s.pins = vec!["sturdy".into(), "cheap".into()];
    assert_eq!(ids(&models(), &s, false), ["cheap"]);
}

#[test]
fn pins_filter_before_the_numbers_are_looked_at() {
    // the pinned candidate lacks the column: the worst known is taken among the pinned only, so it is not charged
    // the price of a candidate that was filtered away
    let c = vec![cand("pin", &[]), cand("pricey", &[("cost", 100.0)])];
    let mut s = spec(&[("cost", -1.0)]);
    s.pins = vec!["pin".into()];
    let r = rank(&c, &s, false).unwrap();
    assert_eq!(r.order[0].score, 0.0);
}

#[test]
fn nothing_left_is_empty_never_a_panic() {
    let e = rank(&[], &spec(&[]), false).unwrap_err();
    assert!(e.why.contains("nothing is offered"), "{e:?}");
    let mut s = spec(&[]);
    s.exclude = vec!["sturdy".into(), "cheap".into(), "flashy".into()];
    let e = rank(&models(), &s, false).unwrap_err();
    assert!(e.why.contains("excluded"), "{e:?}");
    let only_blank = vec![cand("", &[])];
    assert!(rank(&only_blank, &spec(&[]), true).is_err());
}

#[test]
fn a_duplicate_or_nameless_candidate_is_ignored_with_a_note() {
    let c = vec![cand("a", &[("x", 1.0)]), cand("a", &[("x", 9.0)]), cand("", &[("x", 9.0)])];
    let r = rank(&c, &spec(&[("x", 1.0)]), false).unwrap();
    assert_eq!(r.order.len(), 1);
    assert_eq!(r.order[0].score, 1.0, "the first of a twin counts");
    assert_eq!(r.notes.len(), 1);
    assert!(r.notes[0].starts_with("2 candidate(s)"), "{:?}", r.notes);
}

// ---- election ----

#[test]
fn cost_low_counts_only_at_election() {
    // reliability alone favours good; cost_low must not move a bound thread, and must move an election
    let c = vec![cand("good", &[("reliability", 0.95), ("cost", 1.0)]), cand("ok", &[("reliability", 0.7), ("cost", 0.1)])];
    let mut s = spec(&[("reliability", 1.0)]);
    s.cost_low = true;
    assert_eq!(ids(&c, &s, false), ["good", "ok"], "a bound thread keeps weighing as before");
    assert_eq!(ids(&c, &s, true), ["ok", "good"]);
    let off = Spec { cost_low: false, ..s.clone() };
    assert_eq!(ids(&c, &off, true), ["good", "ok"], "without the flag an election does not weigh cost either");
}

#[test]
fn cost_low_weighs_cost_at_least_minus_one_and_a_heavier_weight_stays_heavier() {
    let c = vec![cand("a", &[("cost", 2.0)])];
    let mut s = spec(&[("cost", -0.1)]);
    s.cost_low = true;
    assert_eq!(rank(&c, &s, true).unwrap().order[0].score, -2.0);
    assert_eq!(rank(&c, &s, false).unwrap().order[0].score, -0.2);
    s.weights.insert("cost".into(), -5.0);
    assert_eq!(rank(&c, &s, true).unwrap().order[0].score, -10.0);
    s.weights.insert("cost".into(), 3.0);
    assert_eq!(rank(&c, &s, true).unwrap().order[0].score, -6.0, "cost_low turns a reward for cost into a penalty");
}

// ---- determinism ----

#[test]
fn ties_go_to_the_recommended_then_to_the_smaller_id() {
    let mut c = vec![cand("m", &[]), cand("b", &[]), cand("z", &[]), cand("a", &[])];
    assert_eq!(ids(&c, &spec(&[]), false), ["a", "b", "m", "z"]);
    c[2].recommended = true;
    assert_eq!(ids(&c, &spec(&[]), false), ["z", "a", "b", "m"]);
    c[0].recommended = true;
    assert_eq!(ids(&c, &spec(&[]), false), ["m", "z", "a", "b"]);
}

#[test]
fn a_score_apart_by_float_noise_is_a_tie() {
    let c = vec![cand("b", &[("x", 0.1 + 0.2)]), cand("a", &[("x", 0.3)])];
    assert_eq!(ids(&c, &spec(&[("x", 1.0)]), false), ["a", "b"]);
}

#[test]
fn the_order_does_not_depend_on_the_order_offered() {
    let mut state = 12345u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as f64 / (1u64 << 31) as f64
    };
    let c: Vec<Candidate> = (0..40)
        .map(|i| {
            let mut x = cand(&format!("m{i:02}"), &[("reliability", (next() * 4.0).floor() / 4.0)]);
            if next() > 0.3 {
                x.cols.insert("cost".into(), (next() * 3.0).floor());
            }
            x.recommended = next() > 0.8;
            x
        })
        .collect();
    let s = spec(&[("reliability", 1.0), ("cost", -0.2)]);
    let want = rank(&c, &s, false).unwrap();
    for turn in 1..40 {
        let mut shuffled = c.clone();
        shuffled.rotate_left(turn);
        if turn % 3 == 0 {
            shuffled.reverse();
        }
        assert_eq!(rank(&shuffled, &s, false).unwrap(), want, "turn {turn}");
    }
    assert!(want.order.windows(2).all(|w| w[0].score >= w[1].score - 1e-9));
}

#[test]
fn ten_thousand_candidates_rank_in_under_a_tenth_of_a_second() {
    let c: Vec<Candidate> = (0..10_000)
        .map(|i| {
            let mut x = cand(&format!("m{i}"), &[("reliability", f64::from(i % 97) / 97.0)]);
            if i % 3 != 0 {
                x.cols.insert("cost".into(), f64::from(i % 13));
            }
            x
        })
        .collect();
    let mut s = spec(&[("reliability", 1.0), ("cost", -0.1), ("speed", 1.0)]);
    s.exclude = (0..500).map(|i| format!("m{i}")).collect();
    s.pins = (0..3000).map(|i| format!("m{}", i * 3)).collect();
    let mut r = None;
    crate::cpu::assert_within(std::time::Duration::from_millis(100), "ten thousand candidates with pins and exclusions", || crate::cpu::least_of(3, || r = Some(rank(&c, &s, true).unwrap())));
    let r = r.unwrap();
    assert!(!r.order.is_empty() && r.order.len() < 10_000);
    let all = spec(&[("reliability", 1.0), ("cost", -0.1)]);
    let mut r = None;
    crate::cpu::assert_within(std::time::Duration::from_millis(100), "ten thousand candidates, all of them", || crate::cpu::least_of(3, || r = Some(rank(&c, &all, false).unwrap())));
    let r = r.unwrap();
    assert_eq!(r.order.len(), 10_000);
}

#[test]
fn hostile_ids_and_huge_input_are_only_text() {
    let long = "x".repeat(100_000);
    let nasty = ["\u{0}", "a\nb", "💥", "\u{202e}rtl", long.as_str(), " "];
    let c: Vec<Candidate> = nasty.iter().map(|id| cand(id, &[("x", 1.0)])).collect();
    let mut s = spec(&[("x", 1.0)]);
    s.pins = vec![long.clone(), "💥".into()];
    s.exclude = vec![nasty[1].to_string()];
    let r = rank(&c, &s, true).unwrap();
    assert_eq!(r.order.len(), 2);
    let dup: Vec<Candidate> = (0..10_000).map(|_| cand(&"y".repeat(1000), &[])).collect();
    let r = rank(&dup, &spec(&[]), false).unwrap();
    assert_eq!(r.notes, ["9999 candidate(s) offered twice or without an id — ignored"]);
    assert_eq!(r.order.len(), 1);
}

#[test]
fn a_clear_lead_needs_a_gap_and_a_lone_candidate_has_one() {
    let two = |a: f64, b: f64| rank(&[cand("a", &[("x", a)]), cand("b", &[("x", b)])], &spec(&[("x", 1.0)]), false).unwrap();
    assert!(two(1.0, 0.5).clear(0.0) && two(1.0, 0.5).clear(0.5));
    assert!(!two(1.0, 0.5).clear(0.6));
    assert!(!two(1.0, 1.0).clear(0.0), "equal scores are no lead, even with min_gap 0");
    assert!(!two(1.0, 1.0 - 1e-12).clear(0.0), "float noise is no lead");
    assert!(rank(&[cand("a", &[])], &spec(&[]), false).unwrap().clear(5.0));
    assert!(two(1.0, 0.5).clear(f64::NAN), "a bad min_gap is no gap at all");
}

// ---- the kind and the base specs ----

fn base_rank() -> Layer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/base/rank");
    let (layer, read) = Layer::from_dir(LayerKind::Pack, "base-rank", &dir);
    assert!(read.is_empty(), "{read:?}");
    layer
}

#[test]
fn the_base_specs_load_clean_and_read_as_specs() {
    let (t, problems) = load(vec![base_rank()]);
    assert!(problems.is_empty(), "{problems:?}");
    let (all, bad) = specs(&t);
    assert!(bad.is_empty(), "{bad:?}");
    assert_eq!(all.keys().map(String::as_str).collect::<Vec<_>>(), ["cheap", "default", "quality"]);
    let d = &all["default"];
    assert!(d.weights["reliability"] > 0.0 && d.weights["cost"] < 0.0, "reliable up, cost down");
    assert!(!d.cost_low && all["cheap"].cost_low);
    assert!(d.min_gap > 0.0 && d.pins.is_empty() && d.exclude.is_empty());
    // the base specs do what they say on the standing numbers
    let c = vec![cand("good", &[("reliability", 0.95), ("cost", 1.0)]), cand("ok", &[("reliability", 0.7), ("cost", 0.1)])];
    assert_eq!(ids(&c, d, true), ["good", "ok"]);
    assert_eq!(ids(&c, &all["cheap"], true), ["ok", "good"]);
    assert_eq!(ids(&c, &all["cheap"], false), ["good", "ok"]);
    // `quality` weighs its own number and nothing of cost: the better record leads however dear it is, and a lead inside the board's spread is no clear pick
    let q = &all["quality"];
    assert_eq!(q.weights.keys().map(String::as_str).collect::<Vec<_>>(), ["quality"]);
    let c = vec![cand("dear", &[("quality", 0.9), ("cost", 9.0)]), cand("cheap", &[("quality", 0.7), ("cost", 0.1)])];
    assert_eq!(ids(&c, q, true), ["dear", "cheap"]);
    assert!(q.min_gap >= 0.06);
}

fn project_with(text: &str) -> Layer {
    layer(LayerKind::Project, "project", &[("rank.toml", text)])
}

/// The operator's own files: the layer a spec is written to (a project layer is never trusted).
fn home_with(text: &str) -> Layer {
    layer(LayerKind::User, "home", &[("rank.toml", text)])
}

#[test]
fn a_rank_row_with_a_wrong_field_is_refused_and_left_out() {
    let rows = "api = 1\n\
        [[rank]]\nid = \"ok\"\nweights = { a = 1 }\npins = [\"x\"]\n\
        [[rank]]\nid = \"words\"\nweights = { a = \"high\" }\n\
        [[rank]]\nid = \"flag\"\ncost_low = \"yes\"\n\
        [[rank]]\nid = \"gap\"\nmin_gap = -1\n\
        [[rank]]\nid = \"extra\"\nbogus = 1\n";
    let (t, problems) = load(vec![home_with(rows)]);
    assert_eq!(refused(&problems).len(), 4, "{problems:?}");
    let (all, bad) = specs(&t);
    assert!(bad.is_empty());
    assert_eq!(all.keys().map(String::as_str).collect::<Vec<_>>(), ["ok"]);
    assert_eq!(all["ok"].weights["a"], 1.0, "a whole number is a weight");
}

#[test]
fn a_spec_that_pins_what_it_excludes_is_read_with_a_warning() {
    let rows = "api = 1\n[[rank]]\nid = \"odd\"\npins = [\"x\"]\nexclude = [\"x\", \"y\"]\n";
    let (t, _) = load(vec![home_with(rows)]);
    let (all, problems) = specs(&t);
    assert_eq!(all["odd"].exclude, ["x", "y"]);
    assert!(mentions(&warned(&problems), &["odd", "exclusion wins"]), "{problems:?}");
}

#[test]
fn a_project_never_sets_a_spec_the_operators_own_files_do() {
    let rows = "api = 1\n[[rank]]\nid = \"default\"\npins = [\"mine\"]\n";
    let (t, _) = Tables::load(vec![base_rank(), project_with(rows)]);
    let (all, _) = specs(&t);
    assert!(all["default"].pins.is_empty(), "an untrusted repository does not steer the election");
    let (t, _) = Tables::load(vec![base_rank(), home_with(rows)]);
    let (all, _) = specs(&t);
    assert_eq!(all["default"].pins, ["mine"]);
    assert!(all["default"].weights.contains_key("reliability"), "fields merge, the pack's weights stay");
}

// ---- the answerer ----

fn opt(id: &str, recommended: bool) -> Opt {
    Opt { id: id.into(), label: format!("the {id}"), recommended }
}

fn point(id: &str, rank: Option<&str>) -> AskPoint {
    AskPoint {
        id: id.into(),
        doc: "a test point".into(),
        shape: ShapeKind::Choice,
        impact: Impact::Low,
        data: DataClass::Public,
        chain: vec![],
        fallback: Fallback::Rank,
        rank: rank.map(String::from),
        breaker: true,
    }
}

fn ask(point: &str, shape: AskShape) -> Question {
    Question {
        ask: "t1".into(),
        point: point.into(),
        asker: "t0".into(),
        thread: None,
        text: "which one?".into(),
        shape,
        goal: "".into(),
        act: None,
    }
}

fn choice(opts: &[(&str, bool)]) -> AskShape {
    AskShape::Choice { options: opts.iter().map(|(i, r)| opt(i, *r)).collect() }
}

/// The standing numbers per option, whatever point asks.
struct Standing(BTreeMap<String, Vec<(&'static str, f64)>>);

impl Columns for Standing {
    fn columns(&self, _point: &str, option: &str) -> BTreeMap<String, f64> {
        self.0.get(option).into_iter().flatten().map(|(c, v)| (c.to_string(), *v)).collect()
    }
}

fn standing(rows: &[(&str, Vec<(&'static str, f64)>)]) -> Box<dyn Columns> {
    Box::new(Standing(rows.iter().map(|(o, c)| (o.to_string(), c.clone())).collect()))
}

fn answerer(columns: Box<dyn Columns>) -> RankAnswerer {
    let (t, _) = load(vec![base_rank()]);
    let (all, _) = specs(&t);
    let points = [point("who:work", None), point("who:cheap", Some("cheap")), point("who:lost", Some("nope"))];
    RankAnswerer::new(all, &points, columns)
}

fn pick(o: &Offer) -> (&str, f32, &str) {
    match o {
        Offer::Answer { answer: Answer::Pick { id }, conf, why } => (id, *conf, why),
        other => panic!("not a pick: {other:?}"),
    }
}

#[test]
fn the_answerer_is_the_rank_answerer_of_kind_rank() {
    let a = answerer(Box::new(NoColumns));
    assert_eq!(a.id(), "rank");
    assert_eq!(a.kind(), Kind::Rank);
}

#[test]
fn without_numbers_the_recommended_comes_first_then_the_smallest_id_with_little_confidence() {
    let a = answerer(Box::new(NoColumns));
    let o = a.offer(&ask("who:work", choice(&[("pi", false), ("omp", false), ("zed", false)])));
    let (id, conf, why) = pick(&o);
    assert_eq!((id, conf < 0.5), ("omp", true), "{o:?}");
    assert!(why.contains("no clear lead"), "{why}");
    let o = a.offer(&ask("who:work", choice(&[("pi", false), ("omp", false), ("zed", true)])));
    assert_eq!(pick(&o).0, "zed");
}

#[test]
fn with_numbers_the_head_of_the_ranking_is_picked_and_trusted_when_it_leads() {
    let a = answerer(standing(&[("pi", vec![("reliability", 0.9), ("cost", 0.5)]), ("omp", vec![("reliability", 0.6), ("cost", 0.1)])]));
    let o = a.offer(&ask("who:work", choice(&[("omp", true), ("pi", false)])));
    let (id, conf, why) = pick(&o);
    assert_eq!(id, "pi", "the numbers beat the recommendation");
    assert!(conf >= 0.8 && why.starts_with("weights: "), "{o:?}");
}

#[test]
fn a_gap_under_min_gap_is_a_tie_with_little_confidence() {
    let a = answerer(standing(&[("pi", vec![("reliability", 0.80)]), ("omp", vec![("reliability", 0.79)])]));
    let o = a.offer(&ask("who:work", choice(&[("omp", false), ("pi", false)])));
    assert_eq!(pick(&o).0, "pi");
    assert!(pick(&o).1 < 0.5);
}

#[test]
fn a_lone_option_is_picked_with_confidence() {
    let a = answerer(Box::new(NoColumns));
    let o = a.offer(&ask("who:work", choice(&[("only", false)])));
    assert_eq!(pick(&o).0, "only");
    assert!(pick(&o).1 >= 0.8);
}

#[test]
fn the_point_names_the_spec_that_is_used_and_an_unknown_name_falls_back_to_default() {
    let a = answerer(standing(&[("pi", vec![("reliability", 0.95), ("cost", 1.0)]), ("omp", vec![("reliability", 0.7), ("cost", 0.1)])]));
    let options = || choice(&[("omp", false), ("pi", false)]);
    assert_eq!(pick(&a.offer(&ask("who:work", options()))).0, "pi");
    assert_eq!(pick(&a.offer(&ask("who:cheap", options()))).0, "omp", "cheap counts cost at election");
    assert_eq!(pick(&a.offer(&ask("who:lost", options()))).0, "pi", "an unknown spec is the default one");
    assert_eq!(pick(&a.offer(&ask("who:unlisted", options()))).0, "pi");
}

#[test]
fn the_election_switch_turns_cost_low_off() {
    let mut a = answerer(standing(&[("pi", vec![("reliability", 0.95), ("cost", 1.0)]), ("omp", vec![("reliability", 0.7), ("cost", 0.1)])]));
    let q = ask("who:cheap", choice(&[("omp", false), ("pi", false)]));
    assert!(a.election);
    assert_eq!(pick(&a.offer(&q)).0, "omp");
    a.election = false;
    assert_eq!(pick(&a.offer(&q)).0, "pi");
}

#[test]
fn the_columns_provider_is_told_the_point_and_the_option() {
    use std::sync::{Arc, Mutex};
    type Calls = Arc<Mutex<Vec<(String, String)>>>;
    struct Spy(Calls);
    impl Columns for Spy {
        fn columns(&self, point: &str, option: &str) -> BTreeMap<String, f64> {
            self.0.lock().unwrap().push((point.into(), option.into()));
            BTreeMap::new()
        }
    }
    let calls = Calls::default();
    let a = answerer(Box::new(Spy(calls.clone())));
    a.offer(&ask("who:work", choice(&[("b", false), ("a", false)])));
    assert_eq!(*calls.lock().unwrap(), [("who:work".to_string(), "b".to_string()), ("who:work".to_string(), "a".to_string())]);
}

#[test]
fn the_answerer_passes_on_every_shape_that_is_not_a_choice() {
    let a = answerer(Box::new(NoColumns));
    for shape in [AskShape::YesNo, AskShape::Text, AskShape::Approval { action: "rm -rf x".into(), risk: 90 }] {
        match a.offer(&ask("who:work", shape.clone())) {
            Offer::Pass { why } => assert!(why.contains("choice"), "{why}"),
            other => panic!("{shape:?} -> {other:?}"),
        }
    }
}

#[test]
fn an_empty_or_unusable_offered_set_is_a_pass_never_a_failure() {
    let a = answerer(Box::new(NoColumns));
    for opts in [vec![], vec![("", false)], vec![("", false), ("", true)]] {
        match a.offer(&ask("who:work", choice(&opts))) {
            Offer::Pass { why } => assert!(!why.is_empty()),
            other => panic!("{opts:?} -> {other:?}"),
        }
    }
}

#[test]
fn what_the_answerer_picks_always_passes_the_shapes_own_check() {
    let a = answerer(Box::new(NoColumns));
    let names = ["a", "b", "💥", "x y", "", "\u{7}", "longer-id", "a"];
    let mut picked = 0;
    for n in 0..names.len() {
        for start in 0..names.len() {
            let opts: Vec<(&str, bool)> = (0..n).map(|k| (names[(start + k) % names.len()], k == start % 3)).collect();
            let shape = choice(&opts);
            if let Offer::Answer { answer, .. } = a.offer(&ask("who:work", shape.clone())) {
                assert_eq!(shape.check(&answer), Ok(()), "{opts:?} -> {answer:?}");
                picked += 1;
            }
        }
    }
    assert!(picked > 30, "{picked}");
}

#[test]
fn the_answerer_makes_the_same_pick_for_the_same_question() {
    let a = answerer(Box::new(NoColumns));
    let q = ask("who:work", choice(&[("c", false), ("a", false), ("b", true)]));
    let first = a.offer(&q);
    assert_eq!(pick(&first).0, "b");
    for _ in 0..20 {
        assert_eq!(a.offer(&q), first);
    }
}
