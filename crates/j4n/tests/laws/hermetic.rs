//! Tests run hermetically: through `scripts/t`, which gives a run an empty
//! HOME, no agent CLIs, a dead proxy and a memory cap, and says so with `J4N_HERMETIC=1`. A bare `cargo test` could
//! start a real `claude` or write into the real home.

/// A complaint unless `hermetic` is the runner's `1`.
fn violation(hermetic: Option<&str>) -> Option<String> {
    match hermetic {
        Some("1") => None,
        other => Some(format!(
            "J4N_HERMETIC is {other:?}, not \"1\" — run the tests through scripts/t (or scripts/check.sh), never a bare cargo test"
        )),
    }
}

#[test]
fn the_tests_run_through_the_hermetic_runner() {
    if let Some(complaint) = violation(std::env::var("J4N_HERMETIC").ok().as_deref()) {
        panic!("{complaint}");
    }
}

#[test]
fn only_the_runners_one_is_hermetic() {
    assert_eq!(violation(Some("1")), None);
    for other in [None, Some(""), Some("0"), Some("true")] {
        assert!(violation(other).is_some_and(|text| text.contains("scripts/t")), "{other:?}");
    }
}
