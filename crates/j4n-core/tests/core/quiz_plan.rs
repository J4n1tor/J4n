//! The rule-only decider `plan_shaped` held to the frozen set an outside benchmark wrote
//! (`fixtures/quiz/plan.jsonl`, from its quiz export). The gate never runs the benchmark; it reads this file.
//! The file is never edited by hand: a new export replaces it whole, and the header's count and hash say it is that one.

use j4n_core::front::planning::plan_shaped;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::time::Duration;

const SET: &str = include_str!("../../../../fixtures/quiz/plan.jsonl");

/// The header, and the rows after it (the lines the header's hash is taken over).
fn parts() -> (Value, Vec<&'static str>) {
    let mut lines = SET.lines();
    let head = serde_json::from_str(lines.next().expect("a header line")).expect("the header is JSON");
    (head, lines.collect())
}

/// Every row whose `plan` is not what `plan_shaped` says of its `line`, as a line each: id, line, expected, got.
fn disagree(rows: &[&str]) -> Vec<String> {
    rows.iter()
        .filter_map(|text| {
            let r: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{text:?} is not JSON: {e}"));
            let (id, line) = (r["id"].as_str().expect("id"), r["line"].as_str().expect("line"));
            let want = r["plan"].as_bool().expect("plan");
            let got = plan_shaped(line);
            (want != got).then(|| format!("{id}: {line:?} expected plan={want}, got {got}"))
        })
        .collect()
}

#[test]
fn the_set_is_the_one_the_header_names() {
    let (head, rows) = parts();
    assert_eq!(head["set"], "plan");
    assert_eq!(head["n"].as_u64(), Some(rows.len() as u64), "the header counts the rows: the file was edited, or cut");
    let body: String = rows.iter().map(|l| format!("{l}\n")).collect();
    let sum: String = Sha256::digest(body.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(head["sha256"].as_str(), Some(sum.as_str()), "the rows are not the ones the benchmark wrote: replace the file by a new export, never edit it");
    let ids: std::collections::BTreeSet<_> = rows.iter().map(|r| serde_json::from_str::<Value>(r).unwrap()["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(ids.len(), rows.len(), "an id is there once");
    let yes = rows.iter().filter(|r| serde_json::from_str::<Value>(r).unwrap()["plan"] == true).count();
    assert!(yes > rows.len() / 4 && yes < rows.len() * 3 / 4, "both sides are drawn: {yes} of {}", rows.len());
}

#[test]
fn every_line_of_the_set_gets_its_class_from_plan_shaped() {
    let (_, rows) = parts();
    let bad = disagree(&rows);
    assert!(bad.is_empty(), "{} of {} rows disagree:\n{}", bad.len(), rows.len(), bad.join("\n"));
}

#[test]
fn the_law_names_a_row_that_disagrees() {
    let rows = [r#"{"id": "plan-x1", "line": "plan the cache rewrite", "plan": false, "shape": "plan-asks"}"#, r#"{"id": "plan-x2", "line": "plan the cache rewrite", "plan": true, "shape": "plan-asks"}"#];
    let bad = disagree(&rows);
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].starts_with("plan-x1: \"plan the cache rewrite\" expected plan=false, got true"), "{bad:?}");
}

#[test]
fn the_law_runs_in_under_two_seconds_of_cpu_whatever_the_load() {
    let (_, rows) = parts();
    // the CPU this thread spends, not the wall clock: a loaded machine does not fail it
    crate::cpu::assert_within(Duration::from_secs(2), "the rows of the plan", || crate::cpu::least_of(3, || assert!(disagree(&rows).is_empty())));
}
