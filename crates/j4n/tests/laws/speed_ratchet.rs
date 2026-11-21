//! The speed ratchet as a law: every number the design asks for is a bench in
//! `crates/j4n/benches/tui_speed.rs`, each has a ceiling in `fixtures/tui-bench.toml` that is at most what the design asks, and the gate runs the
//! bench. A ceiling is lowered by the bench itself (`-- --ratchet`) and raised only by hand, in a step that says why.

use crate::support::{assert_clean, read, root};

/// The `(name, microseconds)` pairs the bench holds itself to, as they are written in its source.
fn specs(bench: &str) -> Vec<(String, u64)> {
    bench
        .lines()
        .filter_map(|l| {
            let l = l.trim().strip_prefix("(\"")?;
            let (name, rest) = l.split_once("\", ")?;
            let us = rest.strip_suffix("),")?.replace('_', "").parse().ok()?;
            Some((name.to_string(), us))
        })
        .collect()
}

fn violations(bench: &str, fixture: &str, gate: &str) -> Vec<String> {
    let mut found = Vec::new();
    let ceilings: toml::Table = match fixture.parse() {
        Ok(t) => t,
        Err(e) => return vec![format!("fixtures/tui-bench.toml is not TOML: {e}")],
    };
    let ceilings: Vec<(String, i64)> = ceilings.get("ceiling_us").and_then(toml::Value::as_table).map(|t| t.iter().map(|(k, v)| (k.clone(), v.as_integer().unwrap_or(-1))).collect()).unwrap_or_default();
    let specs = specs(bench);
    if specs.is_empty() {
        found.push("the bench lists no spec numbers (`(\"name\", microseconds),`)".to_string());
    }
    for (name, spec) in &specs {
        match ceilings.iter().find(|(k, _)| k == name) {
            None => found.push(format!("`{name}` has no ceiling in fixtures/tui-bench.toml")),
            Some((_, c)) if *c <= 0 => found.push(format!("`{name}`: a ceiling is a whole number of microseconds above 0")),
            Some((_, c)) if *c as u64 > *spec => found.push(format!("`{name}`: the ceiling {c} µs is above the {spec} µs the design asks")),
            Some(_) => {}
        }
    }
    for (name, _) in &ceilings {
        if !specs.iter().any(|(k, _)| k == name) {
            found.push(format!("`{name}` has a ceiling and no bench"));
        }
    }
    if !gate.contains("--bench tui_speed") {
        found.push("scripts/check.sh does not run the bench (`--bench tui_speed`)".to_string());
    }
    found
}

#[test]
fn every_speed_the_design_asks_for_is_a_bench_with_a_ceiling_at_most_that_and_the_gate_runs_it() {
    let bench = read(&root().join("crates/j4n/benches/tui_speed.rs"));
    let fixture = read(&root().join("fixtures/tui-bench.toml"));
    let gate = read(&root().join("scripts/check.sh"));
    assert_clean("speed ratchet", &violations(&bench, &fixture, &gate));
}

#[test]
fn the_detector_sees_a_missing_ceiling_one_above_the_spec_one_without_a_bench_and_a_gate_that_skips_the_bench() {
    let bench = "const SPEC_US: [(&str, u64); 2] = [\n    (\"frame\", 1_000),\n    (\"key\", 16_000),\n];";
    let gate = "stage \"speed\" scripts/t bench -p j4n --bench tui_speed";
    assert!(violations(bench, "[ceiling_us]\nframe = 600\nkey = 9000\n", gate).is_empty());
    let missing = violations(bench, "[ceiling_us]\nframe = 600\n", gate);
    assert!(missing.len() == 1 && missing[0].contains("`key` has no ceiling"), "{missing:?}");
    let above = violations(bench, "[ceiling_us]\nframe = 1001\nkey = 1\n", gate);
    assert!(above.len() == 1 && above[0].contains("above the 1000"), "{above:?}");
    let zero = violations(bench, "[ceiling_us]\nframe = 0\nkey = 1\n", gate);
    assert!(zero.len() == 1 && zero[0].contains("above 0"), "{zero:?}");
    let extra = violations(bench, "[ceiling_us]\nframe = 1\nkey = 1\nold = 5\n", gate);
    assert!(extra.len() == 1 && extra[0].contains("`old` has a ceiling and no bench"), "{extra:?}");
    let skipped = violations(bench, "[ceiling_us]\nframe = 1\nkey = 1\n", "stage \"tests\" scripts/t test");
    assert!(skipped.len() == 1 && skipped[0].contains("does not run the bench"), "{skipped:?}");
    assert!(!violations("", "[ceiling_us]\n", gate).is_empty(), "a bench with no numbers is a bench that holds nothing");
    assert!(!violations(bench, "not = [toml", gate).is_empty());
}
