//! What the core says of the composer (`composer:strip`, `composer:bind`): the strip above the line — how full the
//! doer's context was, the doer the line goes to and the words that end a step of J4n's own — and what the binder would make of the line being typed while a question is open.

use crate::ask::{Answer, AskShape};
use crate::binder::{self, Bind, OpenAsk};
use serde_json::{Value as Json, json};

/// One task of the thread that works, as the strip counts it: a word for it (its own fraction is not sent, as nothing draws it).
#[derive(Debug, Clone, PartialEq)]
pub struct Work {
    pub label: String,
}

/// What the strip says.
#[derive(Debug, Clone, Default)]
pub struct Strip<'a> {
    /// The context gauge (0–1) of the thread's newest task that reported one.
    pub window: Option<f64>,
    /// The route (`claude · sonnet 5.5`), the thread's binding.
    pub route: &'a str,
    /// While the thread has no doer: the one its first line goes to when that is known before it (`--doer` at the start, or the pick in
    /// Agents › Setup with its account); empty: the election chooses.
    pub next: &'a str,
    /// The word of the switch `lanes`; the strip names it only when it is not `single`.
    pub lanes: &'a str,
    /// The tasks that work now, oldest first; a line that was just sent is one before its task has a frame.
    pub work: Vec<Work>,
    /// What the thread has cost, the way `cost.show` says it; empty before it has cost anything.
    pub cost: String,
    /// `write` or `read`.
    pub mode: &'a str,
    /// How many skills the running tasks were given; the screen drops the count first when the row is short.
    pub skills: usize,
    /// The thread's open plan: items done and all of them; the screen draws it as a thin line.
    pub plan: Option<(usize, usize)>,
    /// How the gauge is drawn (`look.gauge`): `auto` (the circle slice where the strip is short of width, the thin line on a page), `line` (`g3`, the thin line) or `slice` (`g9`, one circle slice); empty is `line`.
    pub form: &'a str,
    /// The words that end J4n's own step that runs in Main, and the step (`{words, what}`, `steps::Front::steps_hint`); null when none can be ended.
    pub skip: Json,
}

/// What the strip's route says while the thread has no doer: the first line elects one. The screen does not draw a strip that says only
/// this; it tells it in the chat's empty state.
pub const NO_DOER: &str = "no doer yet";

/// The strip: the work bar (`work`), the context gauge, the route, this thread's cost and the write mode. `tags` has the word of the
/// switch `lanes` only when it is not `single`; the screen draws a tag only for what is listed. The route is the thread's binding, the
/// core's own word (`route_by`), so the screen does not mark it as a model's guess. `route_by` is `none` for [`NO_DOER`], which the screen draws dim. Null when there is nothing to say: no gauge, route,
/// work or cost.
pub fn strip(s: &Strip) -> Json {
    if s.window.is_none() && s.route.is_empty() && s.work.is_empty() && s.cost.is_empty() && s.plan.is_none() && s.skip.is_null() {
        return Json::Null;
    }
    let tags: Vec<Json> = (s.lanes != "single" && !s.lanes.is_empty()).then(|| json!({ "icon": "tasks", "text": format!("lanes {}", s.lanes) })).into_iter().collect();
    let work: Vec<Json> = s.work.iter().map(|w| json!({ "label": w.label })).collect();
    let mut strip = json!({ "tags": tags, "gauge": s.window, "route": s.route, "route_by": if s.route == NO_DOER { "none" } else { "core" }, "work": work, "cost": s.cost, "mode": s.mode });
    if s.skills > 0 {
        strip["skills"] = json!(s.skills);
    }
    if s.route == NO_DOER && !s.next.is_empty() {
        strip["next"] = json!(s.next);
    }
    if let Some((done, total)) = s.plan {
        strip["plan"] = json!({ "done": done, "total": total });
    }
    if !s.skip.is_null() {
        strip["skip"] = s.skip.clone();
    }
    if matches!(s.form, "slice" | "auto") {
        strip["form"] = json!(s.form);
    }
    strip
}

/// What `text`, typed while the questions `open` wait, would be taken for: `{text, ask, mark, label}` for the question it would answer
/// and the reply in a word, null when it would not answer one (or the binder would ask which).
pub fn guess(text: &str, open: &[OpenAsk]) -> Json {
    let text = text.trim();
    let Bind::Answer { ask, answer } = binder::bind(text, open, None) else { return Json::Null };
    let (mark, label) = match &answer {
        Answer::Yes => ("yes", "yes".to_string()),
        Answer::No => ("no", "no".to_string()),
        Answer::Pick { id } => {
            let label = open.iter().find(|o| o.ask == ask).and_then(|o| match &o.shape {
                AskShape::Choice { options } => options.iter().find(|c| &c.id == id).map(|c| c.label.clone()),
                _ => None,
            });
            ("", label.unwrap_or_else(|| id.clone()))
        }
        Answer::Text { .. } => ("", "reply".to_string()),
    };
    json!({ "text": text, "ask": ask, "mark": mark, "label": label })
}
