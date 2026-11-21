//! Spot checks: a question a machine answered is put again, after its answer took effect, to the next
//! machine answerer of the same point — one that is distinct, and either active or still shadowed — and the two answers are written
//! as `ask.spot`, which rules turn into labels (`agreed`, `disagreed`). The live answer is never touched: it stood when this began,
//! and it stands when this ends. A rule starts it (`packs/base/rules/trust.toml`), at most five an hour and only for a question of
//! `impact = mid`; a question of low impact is not worth the calls, and one of gate impact was answered by two.
//!
//! The question comes from [`Asks::recall`], and goes through the egress Gate as the walk sends it; a spot check that cannot be put
//! (the question is forgotten, nobody else answers there, the zone is not allowed) ends well and says why, because it checked
//! nothing and found nothing wrong.

use super::Asks;
use crate::answer::{Impact, Kind, Offer, State, StateView};
use crate::ask::Answer;
use crate::change::Actor;
use crate::doer::{Caps, Doer, Probe, Run, RunCtl, RunEv, StartCx};
use crate::log::{Log, New};
use crate::table::Problem;
use serde_json::json;
use std::sync::Arc;

/// The id of the doer a spot-check rule starts.
pub const ID: &str = "spot";
/// How far back in the Log the live answer is looked for.
const LOOK_BACK: u64 = 4000;

pub struct Spot {
    asks: Arc<Asks>,
    log: Log,
}

impl Spot {
    pub fn new(asks: Arc<Asks>, log: Log) -> Spot {
        Spot { asks, log }
    }
}

struct SpotRun;

impl Run for SpotRun {
    fn ctl(&mut self, _: RunCtl) -> Result<(), Problem> {
        Ok(())
    }
}

impl Doer for Spot {
    fn id(&self) -> &str {
        ID
    }

    fn probe(&self) -> Probe {
        Probe { installed: true, version: None, authed: true, detail: String::new() }
    }

    fn caps(&self) -> Caps {
        Caps::default()
    }

    fn start(&self, cx: StartCx) -> Result<Box<dyn Run>, Problem> {
        let (asks, log, task, ask, out) = (self.asks.clone(), self.log.clone(), cx.task.clone(), cx.spec.goal.trim().to_string(), cx.out.clone());
        std::thread::spawn(move || {
            let said = check(&asks, &log, &task, &ask);
            out.send(RunEv::End { answer: Some(said), todo_left: 0 });
        });
        Ok(Box::new(SpotRun))
    }
}

/// The live answer of `ask`: who gave it, and what it was.
fn live(log: &Log, ask: &str) -> Option<(String, Answer)> {
    log.read(log.head().saturating_sub(LOOK_BACK)).filter(|e| e.kind == "ask.answered" && e.data.get("ask").and_then(|a| a.as_str()) == Some(ask)).last().and_then(|e| {
        let by = e.data.get("by")?.as_str()?.to_string();
        let given = serde_json::from_value::<Answer>(e.data.get("given")?.clone()).ok()?;
        Some((by, given))
    })
}

fn check(asks: &Asks, log: &Log, task: &str, ask: &str) -> String {
    let Some(q) = asks.recall(ask) else { return format!("{ask} is forgotten: nothing to check") };
    let Some(point) = asks.points.get(&q.point) else { return format!("there is no ask point `{}`", q.point) };
    if point.impact != Impact::Mid {
        return format!("{} is a question of {} impact: it is not spot-checked", q.point, point.impact.word());
    }
    let Some((first_by, first)) = live(log, ask) else { return format!("no machine answer to {ask} is in the Log") };
    let now = log.clock().now_ms();
    for step in point.chain.iter().filter(|s| s.kind.is_model()) {
        let id = step.id();
        let Some(a) = asks.registry.get(&id).filter(|_| id != first_by) else { continue };
        // a shadowed answerer is fine here, that is what this is for; one whose breaker is open is not asked
        if matches!(asks.state.state_for(&id, &point.id, &a.model(), now), State::Off { .. }) || a.kind() == Kind::Rule {
            continue;
        }
        if let Some(zone) = a.zone()
            && asks.registry.allows(point.data, &zone).is_err()
        {
            continue;
        }
        let Offer::Answer { answer, conf, why } = a.offer(&q) else { continue };
        if q.shape.check(&answer).is_err() {
            continue;
        }
        let agreed = answer == first;
        let first_model = asks.registry.get(&first_by).map(|f| f.model()).unwrap_or_default();
        let data = json!({
            "ask": ask, "point": point.id, "agreed": agreed,
            "first_by": first_by, "first_model": first_model, "first": first.render(),
            "second_by": id, "second_model": a.model(), "second": answer.render(), "conf": conf, "why": why,
        });
        let _ = log.append(New::of("ask.spot", Actor::Doer(ID.into()), data).task(task));
        return format!("{id} {} {first_by} on {}", if agreed { "agrees with" } else { "does not agree with" }, point.id);
    }
    format!("no other machine answerer of {} could be asked", point.id)
}
