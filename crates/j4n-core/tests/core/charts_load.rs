//! The first read of the charts goes back a bounded number of events, and leaves out the day it starts in. (A Log of 200,000 events
//! took 4.9 s to read whole in a debug build; the bound is `KEEP`, 100,000, and this test uses a small one.)

use crate::world::*;
use j4n_core::change::Actor;
use j4n_core::clock::Clock;
use j4n_core::front::charts::Charts;
use j4n_core::log::New;
use serde_json::json;

const DAY: u64 = 86_400_000;

fn fill(log: &j4n_core::log::Log, clock: &std::sync::Arc<j4n_core::clock::ManualClock>, n: usize) {
    for _ in 0..n {
        clock.advance(60_000);
        log.append(
            New::of("run.metrics", Actor::Doer("claude".into()), json!({"usd": 0.5, "tokens_in": 10, "tokens_out": 5}))
                .task("t1"),
        )
        .unwrap();
    }
    log.sync();
}

fn usd(c: &Charts, now: u64) -> Vec<f64> {
    c.spend(now)
        .map(|v| v["series"]["usd"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect())
        .unwrap_or_default()
}

#[test]
fn a_long_log_is_counted_from_its_newest_events_and_the_day_it_starts_in_is_left_out() {
    let s = Scratch::new("charts-load");
    let (log, clock) = open_log(&s);
    // 3,000 events a minute apart: two days and a bit, from 1 s past the epoch
    fill(&log, &clock, 3_000);
    let now = clock.now_ms();
    let (mut all, mut last) = (Charts::default(), Charts::default());
    all.load_last(&log, 10_000);
    last.load_last(&log, 1_000);
    let total: f64 = usd(&all, now).iter().sum();
    assert!((total - 1_500.0).abs() < 1e-6, "a Log shorter than the bound is counted whole: {total}");
    // the last 1,000 events (16 h 40 min) are counted from the day they start in, which is the one before: dropped as partial
    let part = usd(&last, now);
    assert!(part.iter().sum::<f64>() < total, "{part:?}");
    assert!(part.len() < usd(&all, now).len(), "the first day of a cut read is not drawn: {part:?}");
    assert!(now / DAY >= 2);
}
