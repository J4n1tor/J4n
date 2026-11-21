//! What the loop needs to decide who answers a question: the ask points, the answerers that are
//! installed, how far each is trusted, and the Gate. Without an [`Asks`] every question goes to the operator, as it did
//! before the chain existed.

use crate::answer::chain::Registry;
use crate::answer::probe::Prober;
use crate::answer::state::Answerers;
use crate::answer::{Answerer, AskPoint, Egress, Kind, Offer, Question};
use crate::change::Actor;
use crate::gate::{Action, Verb};
use crate::schema::DataClass;
use crate::ask::Answer;
use crate::gate::{Gate, Verdict, check::DEFAULT, inside::Inside};
use std::collections::BTreeMap;

/// The id of the answerer that is the Gate.
pub const GATE: &str = "rule:gate";

/// Why an effect the Gate has no rule for is put to the operator.
pub const NOT_ALLOWED: &str = "nothing allows it yet";

/// The id a yes given by `--approval full` carries.
pub const FULL: &str = "rule:approval-full";

/// The ask points by id, the answerers and their state, the Gate, and whether anyone is there to be asked.
pub struct Asks {
    pub points: BTreeMap<String, AskPoint>,
    pub registry: Registry,
    pub state: Answerers,
    pub gate: Option<Gate>,
    /// No operator is there: a question that would wait for them takes the point's fallback instead.
    pub headless: bool,
    /// The operator said yes to everything the Gate does not block (`--approval full`): an approval that would have fallen back
    /// to `no` for want of an operator is a yes, by [`FULL`].
    pub full: bool,
    /// The signed charters: an approval that would wait for the operator, with the operator there to wait, is answered by the one that covers it.
    pub charters: Option<crate::charter::Charters>,
    /// What puts the probe cases to the machine answerers (`:probe`, and at start).
    pub prober: Option<std::sync::Arc<Prober>>,
    /// What each option of each point means, which an asker that offers bare ids is given (`ask_point` `says`).
    pub says: crate::answer::Says,
    /// With no operator there, how long a line waits for the probes of a decider or model in the chain of its point that have not
    /// been proved (zero: it does not wait, and the answerer is skipped until they are).
    /// `probe.wait` in milliseconds, as the settings say now (an edit tunes it).
    probe_wait: std::sync::atomic::AtomicU64,
    /// The last questions the chains were walked for, so a spot check can put one again after the live answer took effect.
    recent: std::sync::Mutex<std::collections::VecDeque<Question>>,
}

/// How many questions are remembered for spot checks.
const RECENT: usize = 64;

impl Asks {
    pub fn new(points: Vec<AskPoint>, mut registry: Registry, state: Answerers) -> Asks {
        let points = points.into_iter().map(|p| (p.id.clone(), p)).collect();
        registry.register(std::sync::Arc::new(GateAnswerer(None)));
        Asks { points, registry, state, gate: None, headless: false, full: false, charters: None, prober: None, says: Default::default(), probe_wait: Default::default(), recent: Default::default() }
    }

    /// Let the Gate answer approvals that carry the effect they ask about.
    pub fn with_gate(mut self, gate: Gate) -> Asks {
        self.registry.register(std::sync::Arc::new(GateAnswerer(Some(gate.clone()))));
        self.registry = std::mem::take(&mut self.registry).with_egress(std::sync::Arc::new(gate.clone()));
        self.gate = Some(gate);
        self
    }

    /// Remember a question that a chain is walked for.
    pub fn remember(&self, q: &Question) {
        let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|r| r.ask != q.ask);
        recent.push_back(q.clone());
        while recent.len() > RECENT {
            recent.pop_front();
        }
    }

    /// The question of `ask`, while it is still remembered.
    pub fn recall(&self, ask: &str) -> Option<Question> {
        self.recent.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|q| q.ask == ask).cloned()
    }

    pub fn with_prober(mut self, prober: Prober) -> Asks {
        self.prober = Some(std::sync::Arc::new(prober));
        self
    }

    pub fn with_says(mut self, says: crate::answer::Says) -> Asks {
        self.says = says;
        self
    }

    /// How long a question waits for a probe that is running for its point (`probe.wait`).
    pub fn probe_wait(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.probe_wait.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn with_probe_wait(self, wait: std::time::Duration) -> Asks {
        self.probe_wait.store(wait.as_millis() as u64, std::sync::atomic::Ordering::SeqCst);
        self
    }

    /// Let signed charters answer the approvals they cover. Only where there is a screen: a run with no one to ask denies.
    pub fn with_charters(mut self, charters: crate::charter::Charters) -> Asks {
        self.charters = Some(charters);
        self
    }

    pub fn headless(mut self, yes: bool) -> Asks {
        self.headless = yes;
        self
    }

    /// Whether `approval.inside` is on for these settings: never in a run that denies every ask (no operator, no `--approval full`), which keeps to
    /// what the rules allow.
    pub fn inside_allowed(&self, settings: &crate::setting::Settings) -> bool {
        settings.flag("approval.inside", false) && !(self.headless && !self.full)
    }

    /// Say yes to the approvals nobody blocked. There is no one to ask then, so it also makes the run headless.
    pub fn full(mut self, yes: bool) -> Asks {
        self.full = yes;
        self.headless |= yes;
        self
    }
}

/// The Gate as a rule in a chain (`rule:gate`): an approval whose effect a rule or a grant allows is a yes, one the Gate blocks
/// is a no, and anything else is passed on to the operator. "Nobody objected" (no rule had a say) is not a yes: the doer asked
/// because it needs a yes from someone.
pub struct GateAnswerer(pub Option<Gate>);

impl Answerer for GateAnswerer {
    fn id(&self) -> &str {
        GATE
    }

    fn kind(&self) -> Kind {
        Kind::Rule
    }

    fn offer(&self, q: &Question) -> Offer {
        let (Some(gate), Some(act)) = (&self.0, &q.act) else { return Offer::Pass { why: "no effect to check".into() } };
        let said = |answer, why: String| Offer::Answer { answer, conf: 1.0, why };
        // under `approval.inside`, what stays inside the project is a yes of the Gate's; what is not says why beside the rule's own reason
        let inside = |why: String| match gate.inside(act) {
            Some(Inside::Yes(what)) => said(Answer::Yes, format!("{INSIDE}{what}")),
            Some(other) => Offer::Pass { why: format!("{why} ({} inside: {})", other.word(), other.why()) },
            None => Offer::Pass { why },
        };
        match gate.check(act) {
            Verdict::Block { why, by } => said(Answer::No, format!("{by}: {why}")),
            Verdict::Allow { by } if by != DEFAULT => said(Answer::Yes, format!("allowed by {by}")),
            Verdict::Allow { .. } => match gate.granted(act) {
                Some(id) => said(Answer::Yes, format!("allowed by grant:{id}")),
                None => inside(NOT_ALLOWED.into()),
            },
            Verdict::Ask { why, .. } => inside(why),
        }
    }
}

/// How a yes of the Gate under `approval.inside` begins its reason.
pub const INSIDE: &str = "inside the project: ";

/// The grant an answer of the Gate names in its reason, if it was a grant that allowed it (so a `Once` grant can be spent).
pub(crate) fn grant_in(why: &str) -> Option<&str> {
    let id = why.strip_prefix("allowed by grant:")?;
    (!id.is_empty()).then_some(id)
}

impl Egress for Gate {
    fn check(&self, class: DataClass, zone: &str) -> Verdict {
        let mut act = Action::new(Verb::Egress, Actor::Core, zone);
        act.class = Some(class);
        Gate::check(self, &act)
    }
}

impl crate::setting::Tune for Asks {
    fn tune(&self, settings: &crate::setting::Settings) {
        let wait = std::time::Duration::from_secs_f64(settings.num("probe.wait", 30.0).max(0.0));
        self.probe_wait.store(wait.as_millis() as u64, std::sync::atomic::Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            gate.set_inside(self.inside_allowed(settings));
        }
    }
}
