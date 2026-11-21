//! The turn-path ratchet (law 15 is proposed and not in the charter yet). For a follow-up
//! line the Brief's tokens and sections, the decision rows raised before the doer starts and the own agent's fixed
//! context per model call may not grow, and may not fall without a record: `fixtures/turn-path/budget.toml` holds each
//! ceiling and every ceiling it has had, each with the reason it moved. Ceilings fall; one rises only in an entry that says,
//! besides what changed (`why`), why the ceiling could not stay (`rise = "…"`, not blank).
//!
//! The measured values come later, from the turn-path scenario (the third line of a thread through the real binary
//! with a scripted doer; decision rows, Brief tokens and sections and the fixed context),
//! which judges each with `judge` and fails on `violation`. Until then only the data and the rule
//! are tested.

use crate::support::{assert_clean, read, root};
use std::cmp::Ordering;
use std::collections::BTreeSet;
use toml::{Table, Value};

/// The numbers the ratchet holds, each with the first ceiling it ever had. A row deleted from the file, or a first ceiling
/// rewritten together with the ceiling, would loosen the ratchet; raising one is an edit of this list that says why.
const METRICS: [(&str, i64); 4] =
    [("brief_tokens", 270), ("brief_sections", 13), ("decision_rows", 3), ("fixed_context_tokens", 1500)];

/// What a measured value says about its ceiling.
#[derive(Debug, PartialEq)]
enum Verdict {
    Hold,
    Grew,
    FellUnrecorded,
}

/// The ratchet: a value holds at its ceiling, grew above it and fell below it.
fn judge(measured: u64, ceiling: u64) -> Verdict {
    match measured.cmp(&ceiling) {
        Ordering::Equal => Verdict::Hold,
        Ordering::Greater => Verdict::Grew,
        Ordering::Less => Verdict::FellUnrecorded,
    }
}

/// The failure a measured value is for the metric `id`; `None` when the ceiling holds.
fn violation(id: &str, measured: u64, ceiling: u64) -> Option<String> {
    match judge(measured, ceiling) {
        Verdict::Hold => None,
        Verdict::Grew => Some(format!("{id}: {measured} is over its ceiling {ceiling}")),
        Verdict::FellUnrecorded => Some(format!(
            "{id}: {measured} is under its ceiling {ceiling} — lower the ceiling and add an entry saying why"
        )),
    }
}

fn budget() -> Table {
    read(&root().join("fixtures/turn-path/budget.toml")).parse().expect("turn-path/budget.toml")
}

/// The ceiling a metric has now.
pub(crate) fn ceiling_of(id: &str) -> u64 {
    let budget = budget();
    let metrics = budget.get("metric").and_then(Value::as_array).expect("metrics");
    let metric = metrics.iter().find(|m| m.get("id").and_then(Value::as_str) == Some(id)).unwrap_or_else(|| panic!("no metric `{id}`"));
    u64::try_from(count(metric, "ceiling").expect("a ceiling")).expect("a count")
}

/// What the ratchet says of the numbers a scenario measured: one line for each that is not exactly at its ceiling.
pub(crate) fn judged(measured: &[(&str, u64)]) -> Vec<String> {
    measured.iter().filter_map(|(id, value)| violation(id, *value, ceiling_of(id))).collect()
}

/// A ceiling or an entry's ceiling: an integer that is not negative.
fn count(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_integer).filter(|n| *n >= 0)
}

/// One `[[metric]]`: its ceiling is its last entry's, and its entries fall, each with a reason.
fn metric_violations(label: &str, metric: &Value) -> Vec<String> {
    let mut violations = Vec::new();
    let entries = metric.get("entry").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let ceiling = count(metric, "ceiling");
    if ceiling.is_none() {
        violations.push(format!("{label}: ceiling is missing or not a non-negative integer"));
    }
    if entries.is_empty() {
        violations
            .push(format!("{label}: no [[metric.entry]] — every ceiling the metric has had is recorded, newest last"));
    }
    for (n, entry) in entries.iter().enumerate() {
        if count(entry, "ceiling").is_none() {
            violations
                .push(format!("{label}: entry {} has no ceiling, or one that is not a non-negative integer", n + 1));
        }
        if entry.get("why").and_then(Value::as_str).is_none_or(|why| why.trim().is_empty()) {
            violations.push(format!("{label}: entry {} has no reason (why)", n + 1));
        }
    }
    let recorded: Vec<Option<i64>> = entries.iter().map(|entry| count(entry, "ceiling")).collect();
    if let (Some(ceiling), Some(Some(last))) = (ceiling, recorded.last())
        && ceiling != *last
    {
        violations.push(format!("{label}: ceiling {ceiling} differs from its last entry {last}"));
    }
    for (pair, entry) in recorded.windows(2).zip(entries.iter().skip(1)) {
        if let [Some(before), Some(after)] = pair {
            // a rise is allowed once the entry that makes it says why the ceiling could not stay
            let rise = entry.get("rise").and_then(Value::as_str).is_some_and(|rise| !rise.trim().is_empty());
            if after > before && !rise {
                violations.push(format!("{label}: entries must fall, but {before} is followed by {after} — a rise needs a `rise = \"…\"` saying why the ceiling could not stay"));
            } else if after == before {
                violations.push(format!("{label}: entries must fall, but {before} is followed by {after}"));
            }
        }
    }
    violations
}

/// The structure of a budget file: `api = 1`, every metric of `required` starting at its root ceiling, ids unique and
/// non-empty, each metric sound.
fn budget_violations(budget: &Table, required: &[(&str, i64)]) -> Vec<String> {
    let mut violations = Vec::new();
    if budget.get("api").and_then(Value::as_integer) != Some(1) {
        violations.push("api must be 1".to_owned());
    }
    let metrics = budget.get("metric").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let mut ids = BTreeSet::new();
    for (n, metric) in metrics.iter().enumerate() {
        let id = metric.get("id").and_then(Value::as_str).unwrap_or_default().trim();
        let label = if id.is_empty() { format!("metric {}", n + 1) } else { format!("metric `{id}`") };
        if id.is_empty() {
            violations.push(format!("{label}: id is missing or empty"));
        } else if !ids.insert(id) {
            violations.push(format!("{label}: id is used twice"));
        }
        violations.extend(metric_violations(&label, metric));
    }
    for (id, root) in required {
        let Some(metric) =
            metrics.iter().find(|metric| metric.get("id").and_then(Value::as_str).map(str::trim) == Some(id))
        else {
            violations.push(format!("metric `{id}` is missing — a deleted row would loosen the ratchet"));
            continue;
        };
        let first = metric
            .get("entry")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first())
            .and_then(|entry| count(entry, "ceiling"));
        if first.is_some_and(|first| first != *root) {
            violations.push(format!("metric `{id}`: its first entry is {}, but it started at {root} — raising a ceiling is an edit of METRICS that says why", first.unwrap_or_default()));
        }
    }
    violations
}

#[test]
fn the_budget_file_is_well_formed() {
    assert_clean("turn-path ratchet (L26)", &budget_violations(&budget(), &METRICS));
}

#[test]
fn a_value_equal_to_its_ceiling_holds() {
    assert_eq!(judge(270, 270), Verdict::Hold);
    assert_eq!(judge(0, 0), Verdict::Hold);
    assert_eq!(violation("brief_tokens", 270, 270), None);
}

#[test]
fn a_value_over_its_ceiling_grew() {
    assert_eq!(judge(271, 270), Verdict::Grew);
    assert_eq!(judge(1, 0), Verdict::Grew);
    let message = violation("brief_tokens", 271, 270).expect("a growth fails");
    assert!(message.contains("brief_tokens") && message.contains("271") && message.contains("270"), "{message}");
}

#[test]
fn a_value_under_its_ceiling_fell_without_a_record() {
    assert_eq!(judge(269, 270), Verdict::FellUnrecorded);
    assert_eq!(judge(0, 1), Verdict::FellUnrecorded);
    let message = violation("brief_tokens", 269, 270).expect("a fall fails until it is recorded");
    assert!(message.contains("lower the ceiling and add an entry saying why"), "{message}");
}

/// A `[[metric]]` with its ceiling and entries, as `(ceiling, why)`.
fn metric(id: &str, ceiling: i64, entries: &[(i64, &str)]) -> String {
    let mut text = format!("[[metric]]\nid = \"{id}\"\nceiling = {ceiling}\n");
    for (entry, why) in entries {
        text.push_str(&format!("[[metric.entry]]\nceiling = {entry}\nwhy = \"{why}\"\n"));
    }
    text
}

fn violations_of(text: &str) -> Vec<String> {
    budget_violations(&text.parse().expect("test budget"), &[])
}

fn mentions(violations: &[String], needle: &str) -> bool {
    violations.iter().any(|violation| violation.contains(needle))
}

#[test]
fn a_budget_whose_ceilings_fell_with_reasons_is_clean() {
    let text = format!(
        "api = 1\n{}{}",
        metric("brief_tokens", 200, &[(270, "first"), (240, "the standing section left"), (200, "cards cut")]),
        metric("decision_rows", 3, &[(3, "first")])
    );
    assert!(violations_of(&text).is_empty(), "{:?}", violations_of(&text));
}

#[test]
fn a_budget_that_misses_a_required_metric_is_caught() {
    let text: Table = format!("api = 1\n{}", metric("a", 3, &[(3, "first")])).parse().expect("test budget");
    assert!(budget_violations(&text, &[("a", 3)]).is_empty());
    let violations = budget_violations(&text, &[("a", 3), ("b", 1)]);
    assert!(mentions(&violations, "metric `b` is missing"), "{violations:?}");
}

#[test]
fn a_first_entry_rewritten_to_raise_the_ceiling_is_caught() {
    let text: Table = format!("api = 1\n{}", metric("a", 9, &[(9, "first")])).parse().expect("test budget");
    let violations = budget_violations(&text, &[("a", 3)]);
    assert!(mentions(&violations, "started at 3"), "{violations:?}");
}

#[test]
fn a_wrong_or_missing_api_is_caught() {
    let metrics = metric("a", 3, &[(3, "first")]);
    for text in [format!("api = 2\n{metrics}"), format!("api = \"1\"\n{metrics}"), metrics] {
        assert!(mentions(&violations_of(&text), "api must be 1"), "{text}");
    }
}

#[test]
fn an_empty_or_missing_id_is_caught() {
    let empty = format!("api = 1\n{}", metric("", 3, &[(3, "first")]));
    assert!(mentions(&violations_of(&empty), "id is missing or empty"));
    let blank = format!("api = 1\n{}", metric("  ", 3, &[(3, "first")]));
    assert!(mentions(&violations_of(&blank), "id is missing or empty"));
    let missing = "api = 1\n[[metric]]\nceiling = 3\n[[metric.entry]]\nceiling = 3\nwhy = \"first\"\n";
    assert!(mentions(&violations_of(missing), "id is missing or empty"));
}

#[test]
fn a_repeated_id_is_caught() {
    let text = format!("api = 1\n{}{}", metric("a", 3, &[(3, "first")]), metric("a", 5, &[(5, "first")]));
    assert!(mentions(&violations_of(&text), "metric `a`: id is used twice"));
}

#[test]
fn a_ceiling_that_is_not_its_last_entry_is_caught() {
    let lowered_without_entry = format!("api = 1\n{}", metric("a", 190, &[(270, "first"), (200, "cut")]));
    assert!(mentions(&violations_of(&lowered_without_entry), "ceiling 190 differs from its last entry 200"));
    let raised = format!("api = 1\n{}", metric("a", 300, &[(270, "first")]));
    assert!(mentions(&violations_of(&raised), "ceiling 300 differs from its last entry 270"));
}

#[test]
fn entries_that_do_not_strictly_fall_are_caught() {
    for entries in [[(270, "first"), (270, "again")], [(270, "first"), (300, "up")]] {
        let text = format!("api = 1\n{}", metric("a", entries[1].0, &entries));
        assert!(mentions(&violations_of(&text), "entries must fall"), "{entries:?}");
    }
}

#[test]
fn a_rise_is_clean_only_when_its_entry_says_why_the_ceiling_could_not_stay() {
    let with = "api = 1\n[[metric]]\nid = \"a\"\nceiling = 300\n[[metric.entry]]\nceiling = 270\nwhy = \"first\"\n[[metric.entry]]\nceiling = 300\nwhy = \"a tool\"\nrise = \"the tool needs its schema\"\n";
    assert!(violations_of(with).is_empty(), "{:?}", violations_of(with));
    for rise in ["", "rise = \"\"\n", "rise = \"   \"\n", "rise = true\n"] {
        let text = with.replace("rise = \"the tool needs its schema\"\n", rise);
        assert!(mentions(&violations_of(&text), "a rise needs a `rise"), "{rise:?}");
    }
    // and an entry that stays where it was is no rise
    let same = with.replace("ceiling = 300\nwhy = \"a tool\"", "ceiling = 270\nwhy = \"a tool\"").replace("ceiling = 300\n[[", "ceiling = 270\n[[");
    assert!(mentions(&violations_of(&same), "entries must fall"), "{same}");
}

#[test]
fn an_entry_without_a_reason_is_caught() {
    for why in ["", "   "] {
        let text = format!("api = 1\n{}", metric("a", 200, &[(270, "first"), (200, why)]));
        assert!(mentions(&violations_of(&text), "entry 2 has no reason"), "{why:?}");
    }
    let missing = "api = 1\n[[metric]]\nid = \"a\"\nceiling = 3\n[[metric.entry]]\nceiling = 3\n";
    assert!(mentions(&violations_of(missing), "entry 1 has no reason"));
}

#[test]
fn a_metric_without_entries_is_caught() {
    let text = format!("api = 1\n{}", metric("a", 3, &[]));
    assert!(mentions(&violations_of(&text), "no [[metric.entry]]"));
}

#[test]
fn a_ceiling_that_is_not_a_count_is_caught() {
    let negative = format!("api = 1\n{}", metric("a", -1, &[(-1, "first")]));
    let violations = violations_of(&negative);
    assert!(mentions(&violations, "ceiling is missing or not a non-negative integer"), "{violations:?}");
    assert!(mentions(&violations, "entry 1 has no ceiling"), "{violations:?}");
    let text = "api = 1\n[[metric]]\nid = \"a\"\nceiling = \"3\"\n[[metric.entry]]\nceiling = 3.5\nwhy = \"first\"\n";
    let violations = violations_of(text);
    assert!(mentions(&violations, "ceiling is missing or not a non-negative integer"), "{violations:?}");
    assert!(mentions(&violations, "entry 1 has no ceiling"), "{violations:?}");
}

#[test]
fn a_budget_without_metrics_is_clean_only_when_none_is_required() {
    assert!(violations_of("api = 1\n").is_empty());
    let none: Table = "api = 1\n".parse().expect("test budget");
    assert!(mentions(&budget_violations(&none, &METRICS), "metric `brief_tokens` is missing"));
}
