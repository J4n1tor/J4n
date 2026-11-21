//! The numbers behind the chart fields: the bindings `chart:<id>`, each a `Value`
//! `{x: [label…], series: {name: [number|null…]}}` that `line` and `bars` draw. Read from the Log once (when a chart is first watched)
//! and kept up as events come; what the book and the lanes hold is read when asked.

use super::Front;
use crate::log::{Ev, Log};
use j4n_proto::{Json, json};
use std::collections::BTreeMap;

pub const SPEND: &str = "chart:spend";
pub const WALL: &str = "chart:wall";
pub const BENCH: &str = "chart:bench";
pub const LANES: &str = "chart:lanes";

const DAY_MS: u64 = 86_400_000;
/// The days Spend shows, and the runs of a case Bench averages over.
const DAYS: u64 = 14;
const TRAIL: usize = 5;
/// How many of the newest events the first read goes through: 200,000 events took 4.9 s to read as a list in a debug build, on the
/// thread that serves the screen, and the list held them all. A Log longer than this is counted from here on, and the day it starts in, which
/// is only partly counted, is left out.
const KEEP: u64 = 100_000;

/// The names of the charts, as `:chart <name>` takes them.
pub const NAMES: [&str; 4] = ["spend", "wall", "bench", "lanes"];

/// The binds that are charts.
pub fn is_chart(bind: &str) -> bool {
    matches!(bind, SPEND | WALL | BENCH | LANES)
}

/// What the Log said that a chart is made of: a day's dollars and tokens, and whether each run of a bench case passed.
#[derive(Default)]
pub struct Charts {
    days: BTreeMap<u64, (f64, u64)>,
    bench: Vec<bool>,
    loaded: bool,
    /// The last event counted: one the Log read already gave, and that the server is told of after, is not counted twice.
    last: u64,
}

/// `MM-DD` of a day counted from 1970-01-01 (civil from days, proleptic Gregorian).
fn label(day: u64) -> String {
    let z = day as i64 + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{m:02}-{d:02}")
}

fn series(x: Vec<String>, list: &[(&str, Vec<Json>)]) -> Json {
    json!({ "x": x, "series": list.iter().map(|(n, v)| (n.to_string(), Json::Array(v.clone()))).collect::<serde_json::Map<_, _>>() })
}

impl Charts {
    /// Read the Log once, the first time a chart is asked for.
    pub fn load(&mut self, log: &Log) {
        self.load_last(log, KEEP);
    }

    /// [`Charts::load`] with the number of events to go back.
    pub fn load_last(&mut self, log: &Log, keep: u64) {
        if !std::mem::replace(&mut self.loaded, true) {
            let from = log.head().saturating_sub(keep);
            log.read(from).for_each(|e| self.count(&e));
            if from > 0 {
                self.days.pop_first();
            }
        }
    }

    /// An event of the Log as the server is told of it: counted once the Log has been read.
    pub fn see(&mut self, ev: &Ev) {
        if self.loaded && ev.seq > self.last {
            self.count(ev);
        }
    }

    fn count(&mut self, ev: &Ev) {
        self.last = self.last.max(ev.seq);
        match ev.kind.as_str() {
            "run.metrics" => {
                let d = self.days.entry(ev.at_ms / DAY_MS).or_default();
                d.0 += ev.data["usd"].as_f64().unwrap_or(0.0);
                d.1 += ev.data["tokens_in"].as_u64().unwrap_or(0) + ev.data["tokens_out"].as_u64().unwrap_or(0);
            }
            "bench.case" => self.bench.push(ev.data["pass"].as_bool().unwrap_or(false)),
            _ => {}
        }
    }

    /// Dollars and tokens for each of the last days, from the first day there is anything on.
    pub fn spend(&self, now: u64) -> Option<Json> {
        let (last, first) = (now / DAY_MS, *self.days.keys().next()?);
        let from = first.max(last.saturating_sub(DAYS - 1)).min(last);
        let (mut usd, mut tokens, mut x) = (Vec::new(), Vec::new(), Vec::new());
        for day in from..=last {
            let (u, t) = self.days.get(&day).copied().unwrap_or_default();
            usd.push(json!(u));
            tokens.push(json!(t));
            x.push(label(day));
        }
        Some(series(x, &[("usd", usd), ("tokens", tokens)]))
    }

    /// The share of the last runs that passed, after each run: a wave of how the bench is going.
    fn bench(&self) -> Option<Json> {
        (!self.bench.is_empty()).then(|| {
            let rate = |n: usize| self.bench[n.saturating_sub(TRAIL)..n].iter().filter(|p| **p).count() as f64 / (n - n.saturating_sub(TRAIL)) as f64;
            series((1..=self.bench.len()).map(|n| n.to_string()).collect(), &[("pass", (1..=self.bench.len()).map(|n| json!(rate(n))).collect())])
        })
    }
}

impl Front {
    /// `:chart spend`: the chart, held in the chat as a row of its own (the same row as `/usage`), and what it shows as plain words for a stream
    /// that cannot draw it (`chat.view` keeps them as its `text`).
    pub(super) fn chart_say(&self, thread: &str, bind: &str) {
        let name = bind.strip_prefix("chart:").unwrap_or(bind);
        let text = match self.chart(&mut Charts::default(), bind, thread) {
            Some(v) => plain(name, &v),
            None => format!("{name}: nothing to draw yet"),
        };
        self.write(thread, "chat.view", crate::change::Actor::Core, json!({ "view": bind, "title": name, "text": text }));
    }

    /// The value of a chart binding, or `None` when there is nothing to draw yet (the field says so in a row).
    pub fn chart(&self, charts: &mut Charts, bind: &str, thread: &str) -> Option<Json> {
        charts.load(self.log());
        let now = self.log().clock().now_ms();
        match bind {
            SPEND => charts.spend(now),
            BENCH => charts.bench(),
            WALL => self.wall(now),
            LANES => self.lane_steps(thread),
            _ => None,
        }
    }

    /// How long the tasks of each of the last days took, from start to end: the middle one and the slowest, in seconds.
    fn wall(&self, now: u64) -> Option<Json> {
        let mut days: BTreeMap<u64, Vec<f64>> = BTreeMap::new();
        for r in self.tasks().book().rows() {
            if let (Some(s), Some(e)) = (r.started_ms, r.ended_ms) {
                days.entry(e / DAY_MS).or_default().push(e.saturating_sub(s) as f64 / 1000.0);
            }
        }
        let (last, first) = (now / DAY_MS, *days.keys().next()?);
        let (mut mid, mut top, mut x) = (Vec::new(), Vec::new(), Vec::new());
        for day in first.max(last.saturating_sub(DAYS - 1))..=last {
            let mut v = days.get(&day).cloned().unwrap_or_default();
            v.sort_by(f64::total_cmp);
            let (m, t) = if v.is_empty() { (Json::Null, Json::Null) } else { (json!(v[v.len() / 2]), json!(v[v.len() - 1])) };
            mid.push(m);
            top.push(t);
            x.push(label(day));
        }
        Some(series(x, &[("median", mid), ("slowest", top)]))
    }

    /// The branches each line of the thread has, as bars.
    fn lane_steps(&self, thread: &str) -> Option<Json> {
        let lane = self.lanes()?.lane(thread);
        let lines: Vec<_> = lane.lines.iter().filter(|l| l.total.branches > 0).collect();
        (!lines.is_empty()).then(|| series(lines.iter().map(|l| l.root.id.clone()).collect(), &[("branches", lines.iter().map(|l| json!(l.total.branches)).collect())]))
    }
}

/// A chart's numbers as lines of words: the last values of each series, under the chart's name.
fn plain(name: &str, v: &Json) -> String {
    let x: Vec<&str> = v["x"].as_array().into_iter().flatten().filter_map(Json::as_str).collect();
    let lines = v["series"].as_object().into_iter().flatten().map(|(series, values)| {
        let list: Vec<String> = values.as_array().into_iter().flatten().zip(&x).map(|(n, x)| n.as_f64().map_or(format!("{x} -"), |n| format!("{x} {n:.2}"))).collect();
        format!("  {series}: {}", list.join(", "))
    });
    std::iter::once(name.to_string()).chain(lines).collect::<Vec<_>>().join("\n")
}
