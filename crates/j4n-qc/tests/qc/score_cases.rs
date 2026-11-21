//! The score for display: nothing decides by it, and a number that could not be earned is never shown as clean.

use j4n_qc::pattern::Severity;
use j4n_qc::scan::Mark;
use j4n_qc::score::{Weights, score};
use std::collections::BTreeMap;

fn mark(sev: Severity, p: f64) -> Mark {
    Mark { pattern: "x".into(), file: "a".into(), line: 1, p, severity: sev, tier: 0, answer: String::new() }
}

#[test]
fn it_starts_at_ten_and_each_mark_costs_its_weight_times_its_chance() {
    let w = Weights::default();
    assert_eq!(score(&w, &[], &BTreeMap::new(), false), 10.0);
    assert_eq!(score(&w, &[mark(Severity::High, 1.0)], &BTreeMap::new(), false), 8.0);
    assert_eq!(score(&w, &[mark(Severity::Mid, 1.0), mark(Severity::Low, 1.0), mark(Severity::High, 0.8)], &BTreeMap::new(), false), 6.9);
    assert_eq!(score(&w, &[mark(Severity::High, 0.4)], &BTreeMap::new(), false), 10.0, "under half a chance costs nothing");
    let many: Vec<Mark> = (0..9).map(|_| mark(Severity::High, 1.0)).collect();
    assert_eq!(score(&w, &many, &BTreeMap::new(), false), 0.0, "never under 0");
}

#[test]
fn the_rubrics_hold_it_under_what_they_allow_and_a_failed_check_under_three() {
    let w = Weights::default();
    let rubrics = BTreeMap::from([("fit".to_string(), (3.0, 3.0)), ("scope".to_string(), (1.0, 2.0)), ("consistency".to_string(), (1.0, 2.0))]);
    // mean(1, 0.5, 0.5) = 0.667 → 6.7
    assert_eq!(score(&w, &[], &rubrics, false), 6.7);
    assert_eq!(score(&w, &[], &BTreeMap::new(), true), 3.0);
    assert_eq!(score(&w, &[mark(Severity::Low, 1.0)], &rubrics, true), 3.0);
}

#[test]
fn a_mark_with_exactly_the_least_chance_costs() {
    assert_eq!(score(&Weights::default(), &[mark(Severity::High, 0.5)], &BTreeMap::new(), false), 9.0);
}
