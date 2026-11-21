//! The Setup card: at a start while no agent works or the Decider was never set up, one approval card says where the agent, the
//! Decider and the test stand, and `⏎` opens Agents › Setup. It takes the place of the old welcome card: whether to turn the Decider on
//! is asked on Setup itself, with its key typed on the same page.
//! It is asked once per J4n home, not per project. `open Setup` answers it: the setting `setup.card` is written off in the home's own
//! layer, which every project reads, so no project of this home asks again. `later` (and `esc`) writes nothing, and the next start asks
//! again. The card reads the doctor's look of the programs, so the screen raises it off its own thread.

use super::{Front, Said};
use crate::change::{Actor, Where};
use crate::proposal::{Draft, Step, StepOp};
use crate::value::Value;
use j4n_proto::{AskView, By, Choice, Reply, Shape};
use serde_json::json;

/// The id of the question: the replies of the screen name it.
const ASK: &str = "setup:card";
/// The setting that is on until the card is answered once in this home.
const MARK: &str = "setup.card";

impl Front {
    /// Raise the card when it is due: its setting is on, and no agent works or the Decider is off. Nothing else is done. It waits for the
    /// doers' probes to answer, so a program not checked yet is never taken for one that does not work.
    pub fn setup_card_open(&self) {
        if !self.0.live.flag(MARK, true) || self.0.welcome.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            return;
        }
        let report = self.cfg().setup.as_ref().map(|c| c.report_now(self.main_pick()));
        let steps = self.setup_steps(report.as_ref());
        let step = |id: &str| steps.iter().find(|v| matches!(v, Value::Map(m) if m.get("id") == Some(&Value::text(id))));
        let field = |id: &str, f: &str| step(id).map(|v| v.pointer(&format!("/{f}"))).unwrap_or_default();
        let words = |id: &str| match field(id, "now") {
            Value::Str(s) => s,
            _ => String::new(),
        };
        let (works, decider) = (field("Agent", "done") == Value::Bool(true), field("Decider", "on") == Value::Bool(true));
        if works && decider {
            return;
        }
        let test = match field("Test", "mark") {
            Value::Str(m) if m == "yes" => "passed",
            Value::Str(m) if m == "no" => "failed",
            _ => "not run",
        };
        let decider = if decider { words("Decider") } else { "not set".into() };
        let card = AskView {
            ask: ASK.into(),
            asker: "j4n".into(),
            question: "Set up J4n?".into(),
            shape: Shape::Approval,
            options: vec![Choice { id: "yes".into(), label: "open Setup".into(), recommended: true, by: By::Core, ..Choice::default() }, Choice { id: "no".into(), label: "later".into(), by: By::Core, ..Choice::default() }],
            effect: format!("Agent: {}; Decider: {decider}; Test: {test}", words("Agent")),
            why: "Agents › Setup leads you through an agent that works, its model and the Decider, which answers the small questions so you are asked less, and proves them with one short call. Put off, the card asks again at the next start.".into(),
            ..AskView::default()
        };
        let mut held = self.0.welcome.lock().unwrap_or_else(|e| e.into_inner());
        if held.is_none() && self.write("main", "setup.asked", Actor::Core, json!({ "ask": ASK, "agent": works, "decider": field("Decider", "on") == Value::Bool(true), "test": test })) {
            *held = Some(card);
        }
    }

    /// The card, while it waits (`Down::Asks`).
    pub fn welcome_asks(&self) -> Vec<AskView> {
        self.0.welcome.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }

    /// The operator's reply to the card: `None` when `ask` is not it. `open Setup` marks the home and opens the page; `later` marks nothing.
    pub(super) fn welcome_reply(&self, ask: &str, reply: &Reply) -> Option<Said> {
        if ask != ASK {
            return None;
        }
        if self.0.welcome.lock().unwrap_or_else(|e| e.into_inner()).take().is_none() {
            return Some(Said::Refused("that question is not open any more".into()));
        }
        let yes = matches!(reply, Reply::Yes);
        self.write("main", "setup.answered", Actor::Operator, json!({ "ask": ASK, "answer": if yes { "open" } else { "later" } }));
        if !yes {
            self.note("main", "Setup waits · Agents › Setup has it, and this card asks again at the next start", "dim");
            return Some(Said::Nothing);
        }
        // answered for this home: its own layer says so, below every project's
        let mut row = toml::Table::new();
        row.insert("value".into(), toml::Value::Boolean(false));
        let step = Step { to: Where::Home, file: Some("setup.toml".into()), kind: "setting".into(), id: MARK.into(), op: StepOp::Set(row) };
        let draft = Draft { key: format!("setting:{MARK}"), title: format!("{MARK}: false"), why: "the Setup card was answered".into(), evidence: 1, steps: vec![step], widens: Vec::new() };
        if let Err(why) = self.edit(draft) {
            self.note("main", &format!("the Setup card could not be kept as answered, so it asks again at the next start: {why}"), "warn");
        }
        Some(Said::Open("setup".into()))
    }
}
