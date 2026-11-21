//! `min_conf` from the numbers: the answerer fold keeps how sure an answer was, joins it to
//! the label that comes later, and says — per point whose chain gives the answerer a `min_conf` — how the answers in the band
//! above the line fared against those above it, and the chain with the line moved when the numbers say so.

use crate::fold_cases::event;
use j4n_core::answer::state::{Answerers, BAND, CONF_MAX, CONF_MIN, LOWER_N, RAISE_N};
use j4n_core::fold::Fold;
use j4n_core::value::{Key, Value};
use serde_json::json;
use std::collections::BTreeMap;

const ID: &str = "decider:jev";
const POINT: &str = "intake";

fn fold(min_conf: &str) -> Answerers {
    let a = Answerers::new();
    a.chains(BTreeMap::from([(POINT.to_string(), vec!["rule:intake".to_string(), format!("decider:jev?min_conf={min_conf}"), "human".to_string()])]));
    a
}

struct Feed {
    a: Answerers,
    seq: u64,
}

impl Feed {
    fn new(min_conf: &str) -> Feed {
        Feed { a: fold(min_conf), seq: 0 }
    }

    fn push(&mut self, kind: &str, data: serde_json::Value) {
        self.seq += 1;
        self.a.apply(&event(self.seq, self.seq * 1000, kind, None, data));
    }

    /// An answer with its confidence, and what became of it.
    fn answer(&mut self, n: u64, conf: f64, wrong: bool) {
        let ask = format!("q{n}");
        self.push("ask.answered", json!({ "ask": ask, "point": POINT, "by": ID, "kind": "decider", "conf": conf, "answer": "task" }));
        self.push("label", json!({ "subject_kind": "answer", "subject": ask, "producer": ID, "point": POINT, "weight": if wrong { 1.0 } else { 0.0 } }));
    }

    fn tune(&self) -> Value {
        self.a.read(&[Key::Name(ID.into()), Key::Name("tune".into()), Key::Name(POINT.into())], 0)
    }

    fn at(&self, field: &str) -> Value {
        self.tune().at(&[Key::Name(field.into())])
    }

    fn chain(&self, field: &str) -> Option<Vec<String>> {
        match self.at(field) {
            Value::List(items) => Some(items.into_iter().map(|v| if let Value::Str(s) = v { s } else { String::new() }).collect()),
            _ => None,
        }
    }
}

fn num(v: f64) -> Value {
    Value::num(v)
}

/// `n` answers in the band above 0.6 (confidence 0.64), the first `wrong` of them wrong; then `above` answers above it (0.92), none wrong.
fn band_and_above(f: &mut Feed, n: u64, wrong: u64, above: u64) {
    (1..=above).for_each(|i| f.answer(i, 0.92, false));
    (1..=n).for_each(|i| f.answer(1000 + i, 0.64, i <= wrong));
}

#[test]
fn answers_just_over_the_line_that_are_wrong_twice_as_often_as_the_surer_ones_move_the_line_up_by_a_step() {
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, RAISE_N as u64, 8, 12);
    assert_eq!((f.at("n_band"), f.at("n_above")), (num(RAISE_N as f64), num(12.0)));
    assert_eq!((f.at("wrong_band"), f.at("wrong_above")), (num(0.2), num(0.0)));
    assert_eq!(f.chain("raise"), Some(vec!["rule:intake".into(), "decider:jev?min_conf=0.65".into(), "human".into()]), "the same chain, one number changed");
    assert_eq!(f.chain("lower"), None);
}

#[test]
fn the_numbers_must_be_enough_and_the_difference_must_be_there() {
    // one answer short of enough in the band
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, RAISE_N as u64 - 1, 8, 12);
    assert_eq!(f.chain("raise"), None, "{RAISE_N} are needed in the band");
    // too few above the band to compare with
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, RAISE_N as u64, 8, (RAISE_N / 4) as u64 - 1);
    assert_eq!(f.chain("raise"), None, "a quarter of that above it, at least");
    // none wrong in the band: nothing to raise for
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, RAISE_N as u64, 0, 12);
    assert_eq!(f.chain("raise"), None);
    // wrong as often above as in the band: the line is not the trouble
    let mut f = Feed::new("0.6");
    (1..=12).for_each(|i| f.answer(i, 0.92, i <= 3));
    (1..=RAISE_N as u64).for_each(|i| f.answer(1000 + i, 0.64, i <= 8));
    assert_eq!((f.at("wrong_band"), f.at("wrong_above")), (num(0.2), num(0.25)));
    assert_eq!(f.chain("raise"), None);
    // exactly twice is enough
    let mut f = Feed::new("0.6");
    (1..=20).for_each(|i| f.answer(i, 0.92, i <= 2));
    (1..=RAISE_N as u64).for_each(|i| f.answer(1000 + i, 0.64, i <= 4));
    assert_eq!((f.at("wrong_band"), f.at("wrong_above")), (num(0.1), num(0.1)));
}

#[test]
fn twice_the_rate_above_is_enough_and_less_than_twice_is_not() {
    let mut f = Feed::new("0.6");
    (1..=20).for_each(|i| f.answer(i, 0.92, i <= 1)); // 5 % above
    (1..=RAISE_N as u64).for_each(|i| f.answer(1000 + i, 0.64, i <= 4)); // 10 % in the band
    assert!(f.chain("raise").is_some(), "10 % is twice 5 %");
    let mut f = Feed::new("0.6");
    (1..=20).for_each(|i| f.answer(i, 0.92, i <= 1));
    (1..=RAISE_N as u64).for_each(|i| f.answer(1000 + i, 0.64, i <= 3)); // 7.5 %
    assert_eq!(f.chain("raise"), None, "7.5 % is less than twice 5 %");
}

#[test]
fn the_band_ends_where_the_answers_end_so_a_line_near_the_top_has_nothing_above_it_to_be_compared_with() {
    // a line at 0.85 has a band of 0.85 to 1.0: nothing is above it, so there is no rate to compare with and no proposal
    let mut f = Feed::new("0.85");
    (1..=RAISE_N as u64).for_each(|i| f.answer(i, 0.9, i <= 20));
    (1..=12).for_each(|i| f.answer(100 + i, 0.99, false));
    assert_eq!((f.at("n_band"), f.at("n_above")), (num(RAISE_N as f64 + 12.0), num(0.0)));
    assert_eq!(f.chain("raise"), None);
    // a line at 0.75 has the surer answers above 0.9, and moves to 0.8
    let mut f = Feed::new("0.75");
    (1..=12).for_each(|i| f.answer(i, 0.95, false));
    (1..=RAISE_N as u64).for_each(|i| f.answer(1000 + i, 0.8, i <= 8));
    assert_eq!(f.chain("raise").map(|c| c[1].clone()), Some("decider:jev?min_conf=0.8".into()));
    let _ = CONF_MAX;
}

#[test]
fn answers_none_of_which_was_wrong_in_the_band_let_the_line_come_down_a_step() {
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, LOWER_N as u64, 0, 12);
    assert_eq!(f.chain("lower"), Some(vec!["rule:intake".into(), "decider:jev?min_conf=0.55".into(), "human".into()]));
    assert_eq!(f.chain("raise"), None);
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, LOWER_N as u64 - 1, 0, 12);
    assert_eq!(f.chain("lower"), None, "{LOWER_N} are needed to loosen");
    let mut f = Feed::new("0.6");
    band_and_above(&mut f, LOWER_N as u64, 1, 12);
    assert_eq!(f.chain("lower"), None, "one wrong answer is enough to leave the line where it is");
    let low = format!("{CONF_MIN}");
    let mut f = Feed::new(&low);
    (1..=LOWER_N as u64).for_each(|i| f.answer(i, 0.31, false));
    assert_eq!(f.chain("lower"), None, "no lower than the least");
    let mut f = Feed::new("0.35");
    (1..=LOWER_N as u64).for_each(|i| f.answer(i, 0.36, false));
    assert_eq!(f.chain("lower").map(|c| c[1].clone()), Some("decider:jev?min_conf=0.3".into()));
}

#[test]
fn only_a_deciders_or_a_models_confidence_is_kept_and_only_for_the_ask_it_was_given_for() {
    let mut f = Feed::new("0.6");
    // a rule answered: no confidence to keep; a label for an ask nobody remembers has none either
    f.push("ask.answered", json!({ "ask": "r1", "point": POINT, "by": ID, "kind": "rule", "conf": 0.7 }));
    f.push("label", json!({ "subject_kind": "answer", "subject": "r1", "producer": ID, "point": POINT, "weight": 1.0 }));
    f.push("label", json!({ "subject_kind": "answer", "subject": "never-seen", "producer": ID, "point": POINT, "weight": 1.0 }));
    f.push("ask.answered", json!({ "ask": "d1", "point": POINT, "by": ID, "kind": "decider", "conf": "high" }));
    f.push("label", json!({ "subject_kind": "answer", "subject": "d1", "producer": ID, "point": POINT, "weight": 1.0 }));
    assert_eq!((f.at("n_band"), f.at("n_above")), (num(0.0), num(0.0)), "{:?}", f.tune());
    // below the line is nothing: it was never acted on
    f.answer(5, 0.59, true);
    assert_eq!(f.at("n_band"), num(0.0));
    // the line itself is in the band, and the band ends before 0.15 above it
    f.answer(6, 0.6, false);
    f.answer(7, 0.6 + BAND - 0.001, false);
    f.answer(8, 0.6 + BAND, false);
    assert_eq!((f.at("n_band"), f.at("n_above")), (num(2.0), num(1.0)));
}

#[test]
fn a_point_with_no_min_conf_for_the_answerer_has_nothing_to_tune_and_the_fold_survives_a_snapshot() {
    let mut f = Feed::new("0.6");
    f.a.chains(BTreeMap::from([("other".to_string(), vec!["decider:jev".to_string()]), (POINT.to_string(), vec!["model:light?min_conf=0.7".to_string()])]));
    f.answer(1, 0.8, true);
    assert_eq!(f.a.read(&[Key::Name(ID.into()), Key::Name("tune".into())], 0), Value::Map(Default::default()), "no step of this answerer has a line");
    // saved and loaded, it says what it said
    let mut g = Feed::new("0.6");
    band_and_above(&mut g, RAISE_N as u64, 8, 12);
    let state = g.a.save();
    let mut again = fold("0.6");
    again.load(state).unwrap();
    assert_eq!(again.read(&[Key::Name(ID.into()), Key::Name("tune".into())], 0), g.a.read(&[Key::Name(ID.into()), Key::Name("tune".into())], 0));
}
