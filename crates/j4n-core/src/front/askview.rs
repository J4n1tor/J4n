//! One open question as the operator is shown it (`j4n-proto::AskView`): what was asked, how long it has
//! waited, what the effect touches, what it would change, why the Gate asks, and who made each recommendation. Everything here is read
//! from what the core holds — the question, the effect it carries, the Gate's verdict, the grants it would give; a field the core has
//! nothing true for (a branch, a commit, a diff of a change not yet made) stays empty.

use super::Front;
use crate::ask::AskShape;
use crate::binder::OpenAsk;
use crate::gate::check::DEFAULT;
use crate::gate::{Action, ReadOnly, Verb, Verdict, grant};
use crate::task::NOT_ALLOWED;
use j4n_proto::{AskView, By, Cell, Choice, Row, Shape, Tone};

/// How long a question has waited, as the screen words it: `0:08`, `8:42`, `1:02:03`.
pub fn wait(ms: u64) -> String {
    let s = ms / 1000;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, m, s) => format!("{m}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}

/// What the effect is about, a mark and a word each: the folder it works in, whether that is outside the project, a read that changes
/// nothing, the class of data that would leave, what it costs.
fn facts(act: &Action) -> Vec<Cell> {
    let mut out = Vec::new();
    if let Some(site) = act.site.as_deref().map(|s| s.trim_end_matches('/')).filter(|s| !s.is_empty()) {
        out.push(Cell::with_icon("folder", site.rsplit('/').next().unwrap_or(site), Tone::Dim));
    }
    if act.inside == Some(false) {
        out.push(Cell::with_icon("risky", "outside the project", Tone::Warn));
    }
    if act.read_only != ReadOnly::No {
        out.push(Cell::with_icon("read-only", "read-only", Tone::Dim));
    }
    if act.verb == Verb::Egress
        && let Some(class) = act.class
    {
        out.push(Cell::new(format!("{} data", class.word()), Tone::Dim));
    }
    if act.usd > 0.0 {
        out.push(Cell::new(format!("${:.2}", act.usd), Tone::Dim));
    }
    out
}

/// The files an effect would write, a row each. The change itself (its lines) is not known before it is made, so a row has no `detail`.
fn change(act: &Action) -> Vec<Row> {
    match act.verb {
        Verb::Write | Verb::Apply => vec![Row { key: act.target.clone(), cells: vec![Cell::with_icon("edit", act.target.clone(), Tone::Plain)], ..Row::default() }],
        _ => Vec::new(),
    }
}

/// What a grant of `scope` says on its row for `act`, when it says more than its word: the program, or the folder, it is for.
fn grant_label(scope: grant::Scope, act: &Action) -> String {
    match scope {
        grant::Scope::Program => format!("yes · allow `{} …` in this folder for this session", grant::program_of(act).unwrap_or_default()),
        grant::Scope::Folder => format!("yes · allow writes in {} for this session", grant::folder_of(&act.target).unwrap_or_default()),
        _ => String::new(),
    }
}

/// The question `o` as the operator is shown it at `now_ms`; `None` when its task is gone.
pub fn view(front: &Front, o: &OpenAsk, now_ms: u64) -> Option<AskView> {
    let book = front.tasks().book();
    let spec = book.get(&o.ask)?.spec.ask()?;
    let gate = front.asks().gate.as_ref();
    let verdict = spec.act.as_deref().zip(gate).map(|(act, gate)| (act, gate.check(act)));
    let (shape, options, risk, grants) = match &o.shape {
        AskShape::YesNo => (Shape::YesNo, Vec::new(), 0, Vec::new()),
        AskShape::Text => (Shape::Text, Vec::new(), 0, Vec::new()),
        AskShape::Approval { risk, .. } => {
            // the grants a yes could be kept for each widen what is allowed; the core says so, and the screen asks for the key twice
            let scopes: Vec<_> = verdict.as_ref().zip(gate).map(|((act, v), gate)| gate.offers(act, v)).unwrap_or_default().into_iter().filter(|s| *s != grant::Scope::Once).collect();
            let act = spec.act.as_deref();
            let options = scopes.iter().map(|s| Choice { id: s.word().into(), label: act.map(|a| grant_label(*s, a)).unwrap_or_default(), by: By::Core, widens: true, ..Choice::default() }).collect();
            (Shape::Approval, options, *risk, scopes.iter().map(|s| s.word().to_string()).collect())
        }
        AskShape::Choice { options } => {
            // a pick that is recommended was made by whoever asked: a flow is a rule, any other doer is a doer (the core cannot tell a model
            // from a probe of its own, so it does not say `model`)
            let by = if book.get(&o.asker).is_some_and(|r| r.spec.doer.starts_with("flow:")) { By::Rule } else { By::Doer };
            (Shape::Choice, options.iter().map(|c| Choice { id: c.id.clone(), label: c.label.clone(), recommended: c.recommended, by: if c.recommended { by } else { By::Operator }, ..Choice::default() }).collect(), 0, Vec::new())
        }
    };
    let act = spec.act.as_deref();
    let why = match &verdict {
        Some((_, Verdict::Ask { why, .. })) => why.clone(),
        // no rule asks, and none allows: the Gate's answerer passed it on
        Some((_, Verdict::Allow { by })) if by == DEFAULT => NOT_ALLOWED.to_string(),
        _ => String::new(),
    };
    Some(AskView {
        ask: o.ask.clone(),
        asker: o.asker.clone(),
        question: o.question.clone(),
        shape,
        options,
        grants,
        effect: act.map(|a| format!("{}: {}", a.verb.word(), a.target)).unwrap_or_default(),
        risk,
        wait: wait(now_ms.saturating_sub(o.opened_ms)),
        // a question of a chat is answered from the orchestrator: it says which chat it is of
        facts: book.get(&o.asker).and_then(|r| r.spec.thread).filter(|t| front.chats_on() && front.chat(t).is_some()).map(|t| Cell::new(t, Tone::Dim)).into_iter().chain(act.map(facts).unwrap_or_default()).collect(),
        change: act.map(change).unwrap_or_default(),
        why,
    })
}
