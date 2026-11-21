//! The base pack, compiled in: the files of `packs/base` that are data. The kinds are the core's own
//! (`j4n_core::table`) and the manifest says nothing the binary needs, so they are not here; a test keeps this list and
//! the directory the same.

use j4n_core::pack::{Catalog, Harness, Pack};
use j4n_core::table::{Layer, LayerKind, SourceFile};

/// `(path in the pack, its text)`.
pub const BASE: [(&str, &str); 66] = [
    ("asks/points.toml", include_str!("../../../packs/base/asks/points.toml")),
    ("boxes/bwrap.toml", include_str!("../../../packs/base/boxes/bwrap.toml")),
    ("checks/detect.toml", include_str!("../../../packs/base/checks/detect.toml")),
    ("checks/named.toml", include_str!("../../../packs/base/checks/named.toml")),
    ("connectors/agents.toml", include_str!("../../../packs/base/connectors/agents.toml")),
    ("failures/table.toml", include_str!("../../../packs/base/failures/table.toml")),
    ("folds/decisions.toml", include_str!("../../../packs/base/folds/decisions.toml")),
    ("folds/juards.toml", include_str!("../../../packs/base/folds/juards.toml")),
    ("folds/spend.toml", include_str!("../../../packs/base/folds/spend.toml")),
    ("folds/served.toml", include_str!("../../../packs/base/folds/served.toml")),
    ("gates/egress.toml", include_str!("../../../packs/base/gates/egress.toml")),
    ("gates/modes.toml", include_str!("../../../packs/base/gates/modes.toml")),
    ("gates/paths.toml", include_str!("../../../packs/base/gates/paths.toml")),
    ("gates/shell.toml", include_str!("../../../packs/base/gates/shell.toml")),
    ("commands/main.toml", include_str!("../../../packs/base/commands/main.toml")),
    ("commands/memory.toml", include_str!("../../../packs/base/commands/memory.toml")),
    ("flows/memory.toml", include_str!("../../../packs/base/flows/memory.toml")),
    ("flows/signin.toml", include_str!("../../../packs/base/flows/signin.toml")),
    ("keys/hints.toml", include_str!("../../../packs/base/keys/hints.toml")),
    ("keys/main.toml", include_str!("../../../packs/base/keys/main.toml")),
    ("look/look.toml", include_str!("../../../packs/base/look/look.toml")),
    ("pages/main.toml", include_str!("../../../packs/base/pages/main.toml")),
    ("phrases/ask.toml", include_str!("../../../packs/base/phrases/ask.toml")),
    ("phrases/weigh.toml", include_str!("../../../packs/base/phrases/weigh.toml")),
    ("phrases/commands.toml", include_str!("../../../packs/base/phrases/commands.toml")),
    ("plans/claude.toml", include_str!("../../../packs/base/plans/claude.toml")),
    ("needs/needs.toml", include_str!("../../../packs/base/needs/needs.toml")),
    ("hosts/hosts.toml", include_str!("../../../packs/base/hosts/hosts.toml")),
    ("providers/main.toml", include_str!("../../../packs/base/providers/main.toml")),
    ("plans/sources.toml", include_str!("../../../packs/base/plans/sources.toml")),
    ("sources/catalog.toml", include_str!("../../../packs/base/sources/catalog.toml")),
    ("folds/watch.toml", include_str!("../../../packs/base/folds/watch.toml")),
    ("settings/watch.toml", include_str!("../../../packs/base/settings/watch.toml")),
    ("probes/access.toml", include_str!("../../../packs/base/probes/access.toml")),
    ("probes/chats.toml", include_str!("../../../packs/base/probes/chats.toml")),
    ("probes/complete.toml", include_str!("../../../packs/base/probes/complete.toml")),
    ("probes/fold.toml", include_str!("../../../packs/base/probes/fold.toml")),
    ("probes/inside.toml", include_str!("../../../packs/base/probes/inside.toml")),
    ("probes/intake.toml", include_str!("../../../packs/base/probes/intake.toml")),
    ("probes/librarian.toml", include_str!("../../../packs/base/probes/librarian.toml")),
    ("probes/retry.toml", include_str!("../../../packs/base/probes/retry.toml")),
    ("probes/reflex.toml", include_str!("../../../packs/base/probes/reflex.toml")),
    ("probes/read.toml", include_str!("../../../packs/base/probes/read.toml")),
    ("probes/weigh.toml", include_str!("../../../packs/base/probes/weigh.toml")),
    ("probes/who.toml", include_str!("../../../packs/base/probes/who.toml")),
    ("rank/default.toml", include_str!("../../../packs/base/rank/default.toml")),
    ("rules/check.toml", include_str!("../../../packs/base/rules/check.toml")),
    ("rules/juards.toml", include_str!("../../../packs/base/rules/juards.toml")),
    ("rules/loop.toml", include_str!("../../../packs/base/rules/loop.toml")),
    ("rules/trust.toml", include_str!("../../../packs/base/rules/trust.toml")),
    ("rules/watch.toml", include_str!("../../../packs/base/rules/watch.toml")),
    ("settings/look.toml", include_str!("../../../packs/base/settings/look.toml")),
    ("settings/lanes.toml", include_str!("../../../packs/base/settings/lanes.toml")),
    ("settings/orchestrate.toml", include_str!("../../../packs/base/settings/orchestrate.toml")),
    ("settings/cost.toml", include_str!("../../../packs/base/settings/cost.toml")),
    ("settings/loop.toml", include_str!("../../../packs/base/settings/loop.toml")),
    ("settings/read.toml", include_str!("../../../packs/base/settings/read.toml")),
    ("settings/route.toml", include_str!("../../../packs/base/settings/route.toml")),
    ("settings/tiers.toml", include_str!("../../../packs/base/settings/tiers.toml")),
    ("settings/theme.toml", include_str!("../../../packs/base/settings/theme.toml")),
    ("settings/trust.toml", include_str!("../../../packs/base/settings/trust.toml")),
    ("settings/web.toml", include_str!("../../../packs/base/settings/web.toml")),
    ("settings/work.toml", include_str!("../../../packs/base/settings/work.toml")),
    ("templates/main.toml", include_str!("../../../packs/base/templates/main.toml")),
    ("views/f1.toml", include_str!("../../../packs/base/views/f1.toml")),
    ("views/main.toml", include_str!("../../../packs/base/views/main.toml")),
];

pub fn base() -> Layer {
    Layer::new(LayerKind::Pack, "base-data", BASE.iter().map(|(path, text)| SourceFile { path: (*path).into(), text: (*text).into() }).collect())
}

/// The files of the pack `agent`: the model that works on its own, with the tools J4n carries, and its two loops.
pub const AGENT: [(&str, &str); 2] = [
    ("connectors/own.toml", include_str!("../../../packs/agent/connectors/own.toml")),
    ("flows/loops.toml", include_str!("../../../packs/agent/flows/loops.toml")),
];

/// The files of the pack `code`: the coding harness's flows and the words that start them.
pub const CODE: [(&str, &str); 3] = [
    ("flows/plan-work-validate.toml", include_str!("../../../packs/code/flows/plan-work-validate.toml")),
    ("flows/plan.toml", include_str!("../../../packs/code/flows/plan.toml")),
    ("phrases/flows.toml", include_str!("../../../packs/code/phrases/flows.toml")),
];

fn layer(name: &str, files: &[(&str, &str)]) -> Layer {
    Layer::new(LayerKind::Pack, name, files.iter().map(|(path, text)| SourceFile { path: (*path).into(), text: (*text).into() }).collect())
}

pub fn agent() -> Layer {
    layer("agent-data", &AGENT)
}

pub fn code() -> Layer {
    layer("code-data", &CODE)
}

/// The files of the pack `connect`: the Connect flow and its rules (a program J4n has never seen, connected with no code), and the
/// connectors for agents J4n has a dialect for and the operator may not have.
pub const CONNECT: [(&str, &str); 15] = [
    ("asks/points.toml", include_str!("../../../packs/connect/asks/points.toml")),
    ("commands/connector.toml", include_str!("../../../packs/connect/commands/connector.toml")),
    ("connectors/codex.toml", include_str!("../../../packs/connect/connectors/codex.toml")),
    ("connectors/gemini.toml", include_str!("../../../packs/connect/connectors/gemini.toml")),
    ("connectors/opencode.toml", include_str!("../../../packs/connect/connectors/opencode.toml")),
    ("events/lab.toml", include_str!("../../../packs/connect/events/lab.toml")),
    ("flows/connect-test.toml", include_str!("../../../packs/connect/flows/connect-test.toml")),
    ("flows/connect.toml", include_str!("../../../packs/connect/flows/connect.toml")),
    ("kinds/sniff.toml", include_str!("../../../packs/connect/kinds/sniff.toml")),
    ("pages/connect.toml", include_str!("../../../packs/connect/pages/connect.toml")),
    ("phrases/connect.toml", include_str!("../../../packs/connect/phrases/connect.toml")),
    ("rules/connect.toml", include_str!("../../../packs/connect/rules/connect.toml")),
    ("settings/connect.toml", include_str!("../../../packs/connect/settings/connect.toml")),
    ("sniffs/main.toml", include_str!("../../../packs/connect/sniffs/main.toml")),
    ("views/connect.toml", include_str!("../../../packs/connect/views/connect.toml")),
];

/// The files of the pack `jev`: the deciders and models that answer questions, behind the egress Gate.
pub const JEV: [(&str, &str); 2] = [
    ("connectors/jev.toml", include_str!("../../../packs/jev/connectors/jev.toml")),
    ("models/openrouter.toml", include_str!("../../../packs/jev/models/openrouter.toml")),
];

/// The files of the pack `terminal`: tmux panes as the tools of a model.
pub const TERMINAL: [(&str, &str); 1] = [("connectors/tmux.toml", include_str!("../../../packs/terminal/connectors/tmux.toml"))];

/// The files of the pack `accounts`: several logins of one harness, each in a folder, a window and a route of its own.
pub const ACCOUNTS: [(&str, &str); 9] = [
    ("commands/account.toml", include_str!("../../../packs/accounts/commands/account.toml")),
    ("connectors/browser.toml", include_str!("../../../packs/accounts/connectors/browser.toml")),
    ("events/account.toml", include_str!("../../../packs/accounts/events/account.toml")),
    ("failures/account.toml", include_str!("../../../packs/accounts/failures/account.toml")),
    ("flows/account.toml", include_str!("../../../packs/accounts/flows/account.toml")),
    ("folds/accounts.toml", include_str!("../../../packs/accounts/folds/accounts.toml")),
    ("kinds/account.toml", include_str!("../../../packs/accounts/kinds/account.toml")),
    ("pages/accounts.toml", include_str!("../../../packs/accounts/pages/accounts.toml")),
    ("views/accounts.toml", include_str!("../../../packs/accounts/views/accounts.toml")),
];

/// The files of the pack `bench`: the case kind, the flow that runs one, the results, and two small cases.
pub const BENCH: [(&str, &str); 8] = [
    ("cases/smoke.toml", include_str!("../../../packs/bench/cases/smoke.toml")),
    ("commands/bench.toml", include_str!("../../../packs/bench/commands/bench.toml")),
    ("events/bench.toml", include_str!("../../../packs/bench/events/bench.toml")),
    ("flows/bench.toml", include_str!("../../../packs/bench/flows/bench.toml")),
    ("folds/bench.toml", include_str!("../../../packs/bench/folds/bench.toml")),
    ("kinds/case.toml", include_str!("../../../packs/bench/kinds/case.toml")),
    ("pages/bench.toml", include_str!("../../../packs/bench/pages/bench.toml")),
    ("views/bench.toml", include_str!("../../../packs/bench/views/bench.toml")),
];

/// The files of the pack `shape`: prompt shaping, off until `:shape on`.
pub const SHAPE: [(&str, &str); 9] = [
    ("needs/needs.toml", include_str!("../../../packs/shape/needs/needs.toml")),
    ("events/shape.toml", include_str!("../../../packs/shape/events/shape.toml")),
    ("folds/shape.toml", include_str!("../../../packs/shape/folds/shape.toml")),
    ("kinds/shape.toml", include_str!("../../../packs/shape/kinds/shape.toml")),
    ("phrases/shape.toml", include_str!("../../../packs/shape/phrases/shape.toml")),
    ("rules/shape.toml", include_str!("../../../packs/shape/rules/shape.toml")),
    ("settings/shape.toml", include_str!("../../../packs/shape/settings/shape.toml")),
    ("templates/templates.toml", include_str!("../../../packs/shape/templates/templates.toml")),
    ("texts/personas.toml", include_str!("../../../packs/shape/texts/personas.toml")),
];

/// The files of the pack `qc`: quality control of code, off until `:qc on`.
pub const QC: [(&str, &str); 15] = [
    ("needs/needs.toml", include_str!("../../../packs/qc/needs/needs.toml")),
    ("commands/qc.toml", include_str!("../../../packs/qc/commands/qc.toml")),
    ("events/qc.toml", include_str!("../../../packs/qc/events/qc.toml")),
    ("flows/qc.toml", include_str!("../../../packs/qc/flows/qc.toml")),
    ("folds/qc.toml", include_str!("../../../packs/qc/folds/qc.toml")),
    ("keeps/keep.toml", include_str!("../../../packs/qc/keeps/keep.toml")),
    ("kinds/qc.toml", include_str!("../../../packs/qc/kinds/qc.toml")),
    ("pages/qc.toml", include_str!("../../../packs/qc/pages/qc.toml")),
    ("patterns/tier0.toml", include_str!("../../../packs/qc/patterns/tier0.toml")),
    ("patterns/tier1.toml", include_str!("../../../packs/qc/patterns/tier1.toml")),
    ("rubrics/rubrics.toml", include_str!("../../../packs/qc/rubrics/rubrics.toml")),
    ("rules/qc.toml", include_str!("../../../packs/qc/rules/qc.toml")),
    ("score/score.toml", include_str!("../../../packs/qc/score/score.toml")),
    ("settings/qc.toml", include_str!("../../../packs/qc/settings/qc.toml")),
    ("views/qc.toml", include_str!("../../../packs/qc/views/qc.toml")),
];

/// The files of the pack `contribute`: what this J4n found wrong with J4n and the fixes it carries, kept on this machine.
pub const CONTRIBUTE: [(&str, &str); 7] = [
    ("commands/contribute.toml", include_str!("../../../packs/contribute/commands/contribute.toml")),
    ("events/contribute.toml", include_str!("../../../packs/contribute/events/contribute.toml")),
    ("folds/contribute.toml", include_str!("../../../packs/contribute/folds/contribute.toml")),
    ("pages/contribute.toml", include_str!("../../../packs/contribute/pages/contribute.toml")),
    ("rules/contribute.toml", include_str!("../../../packs/contribute/rules/contribute.toml")),
    ("settings/contribute.toml", include_str!("../../../packs/contribute/settings/contribute.toml")),
    ("views/contribute.toml", include_str!("../../../packs/contribute/views/contribute.toml")),
];

/// The files of the pack `inbox`: the lines typed while a task works, tagged and kept (the setting `inbox`, off until it is turned on).
pub const INBOX: [(&str, &str); 8] = [
    ("commands/inbox.toml", include_str!("../../../packs/inbox/commands/inbox.toml")),
    ("events/inbox.toml", include_str!("../../../packs/inbox/events/inbox.toml")),
    ("folds/inbox.toml", include_str!("../../../packs/inbox/folds/inbox.toml")),
    ("kinds/inbox.toml", include_str!("../../../packs/inbox/kinds/inbox.toml")),
    ("pages/inbox.toml", include_str!("../../../packs/inbox/pages/inbox.toml")),
    ("settings/inbox.toml", include_str!("../../../packs/inbox/settings/inbox.toml")),
    ("views/inbox.toml", include_str!("../../../packs/inbox/views/inbox.toml")),
    ("words/inbox.toml", include_str!("../../../packs/inbox/words/inbox.toml")),
];

/// The files of the pack `ledger`: one report per settled thread, read from the Log (the setting `ledger.mode`, on).
pub const LEDGER: [(&str, &str); 8] = [
    ("needs/needs.toml", include_str!("../../../packs/ledger/needs/needs.toml")),
    ("commands/ledger.toml", include_str!("../../../packs/ledger/commands/ledger.toml")),
    ("events/ledger.toml", include_str!("../../../packs/ledger/events/ledger.toml")),
    ("folds/ledger.toml", include_str!("../../../packs/ledger/folds/ledger.toml")),
    ("pages/ledger.toml", include_str!("../../../packs/ledger/pages/ledger.toml")),
    ("rules/ledger.toml", include_str!("../../../packs/ledger/rules/ledger.toml")),
    ("settings/ledger.toml", include_str!("../../../packs/ledger/settings/ledger.toml")),
    ("views/ledger.toml", include_str!("../../../packs/ledger/views/ledger.toml")),
];

/// The files of the pack `parts`: a document's parts run one at a time, gated and committed (the setting `parts.mode`, off until it is turned on).
pub const PARTS: [(&str, &str); 4] = [
    ("commands/parts.toml", include_str!("../../../packs/parts/commands/parts.toml")),
    ("events/parts.toml", include_str!("../../../packs/parts/events/parts.toml")),
    ("gates/parts.toml", include_str!("../../../packs/parts/gates/parts.toml")),
    ("settings/parts.toml", include_str!("../../../packs/parts/settings/parts.toml")),
];

/// The files of the pack `habits`: show-only detectors of waste (the setting `habits.mode`, show).
pub const HABITS: [(&str, &str); 7] = [
    ("commands/habits.toml", include_str!("../../../packs/habits/commands/habits.toml")),
    ("flows/habits.toml", include_str!("../../../packs/habits/flows/habits.toml")),
    ("folds/habits.toml", include_str!("../../../packs/habits/folds/habits.toml")),
    ("pages/habits.toml", include_str!("../../../packs/habits/pages/habits.toml")),
    ("rules/habits.toml", include_str!("../../../packs/habits/rules/habits.toml")),
    ("settings/habits.toml", include_str!("../../../packs/habits/settings/habits.toml")),
    ("views/habits.toml", include_str!("../../../packs/habits/views/habits.toml")),
];

/// The files of the pack `grow`: what J4n notices about itself and proposes (the operator's yes counted, a machine's confidence measured).
pub const GROW: [(&str, &str); 3] = [
    ("folds/allowed.toml", include_str!("../../../packs/grow/folds/allowed.toml")),
    ("rules/tips.toml", include_str!("../../../packs/grow/rules/tips.toml")),
    ("settings/grow.toml", include_str!("../../../packs/grow/settings/grow.toml")),
];

/// The files of the pack `lanes`: the classes of branch the main lane may start by rule, and under which word of the setting `lanes`.
pub const LANES: [(&str, &str); 2] = [
    ("kinds/branch.toml", include_str!("../../../packs/lanes/kinds/branch.toml")),
    ("rules/branch.toml", include_str!("../../../packs/lanes/rules/branch.toml")),
];

/// The files of the pack `crew`: the kinds `charter`, `link`, `seat` and `seed`, the screens of the crew, the six seats it ships, the board's scores the quality pick starts from, and the switch
/// `crew.on` (off) with the settings beside it.
pub const CREW: [(&str, &str); 10] = [
    ("commands/envoy.toml", include_str!("../../../packs/crew/commands/envoy.toml")),
    ("kinds/charter.toml", include_str!("../../../packs/crew/kinds/charter.toml")),
    ("pages/crew.toml", include_str!("../../../packs/crew/pages/crew.toml")),
    ("views/crew.toml", include_str!("../../../packs/crew/views/crew.toml")),
    ("kinds/link.toml", include_str!("../../../packs/crew/kinds/link.toml")),
    ("kinds/seat.toml", include_str!("../../../packs/crew/kinds/seat.toml")),
    ("kinds/seed.toml", include_str!("../../../packs/crew/kinds/seed.toml")),
    ("seats/seats.toml", include_str!("../../../packs/crew/seats/seats.toml")),
    ("seeds/terminal-bench.toml", include_str!("../../../packs/crew/seeds/terminal-bench.toml")),
    ("settings/crew.toml", include_str!("../../../packs/crew/settings/crew.toml")),
];

/// The files of the pack `guard`: the names of the secret files a task behind a box sees only as their view, what a run may
/// not do once it has read untrusted content, the notice of what the guard stopped, and the five known attacks it names when
/// one matches.
pub const GUARD: [(&str, &str); 4] = [
    ("settings/guard.toml", include_str!("../../../packs/guard/settings/guard.toml")),
    ("gates/taint.toml", include_str!("../../../packs/guard/gates/taint.toml")),
    ("rules/alert.toml", include_str!("../../../packs/guard/rules/alert.toml")),
    ("incidents/known.toml", include_str!("../../../packs/guard/incidents/known.toml")),
];

/// The files of the pack `kit`: the event of a cell of the tool `py`, the setting `kit.mode` (off until it is turned on), the Kit page
/// with `:kit export`, and the rule that proposes turning off a part no cell used. The kernel is carried by `j4n-connect`, which starts it.
pub const KIT: [(&str, &str); 9] = [
    ("needs/needs.toml", include_str!("../../../packs/kit/needs/needs.toml")),
    ("commands/kit.toml", include_str!("../../../packs/kit/commands/kit.toml")),
    ("events/kit.toml", include_str!("../../../packs/kit/events/kit.toml")),
    ("flows/kit.toml", include_str!("../../../packs/kit/flows/kit.toml")),
    ("gates/need.toml", include_str!("../../../packs/kit/gates/need.toml")),
    ("pages/kit.toml", include_str!("../../../packs/kit/pages/kit.toml")),
    ("rules/kit.toml", include_str!("../../../packs/kit/rules/kit.toml")),
    ("settings/kit.toml", include_str!("../../../packs/kit/settings/kit.toml")),
    ("views/kit.toml", include_str!("../../../packs/kit/views/kit.toml")),
];

/// The files of the pack `offload`: the classes of chore a line may be, the switch `offload.on` with its cap and order, and `:offload`.
pub const OFFLOAD: [(&str, &str); 5] = [
    ("needs/needs.toml", include_str!("../../../packs/offload/needs/needs.toml")),
    ("chores/chores.toml", include_str!("../../../packs/offload/chores/chores.toml")),
    ("commands/offload.toml", include_str!("../../../packs/offload/commands/offload.toml")),
    ("kinds/chore.toml", include_str!("../../../packs/offload/kinds/chore.toml")),
    ("settings/offload.toml", include_str!("../../../packs/offload/settings/offload.toml")),
];

/// The files of the pack `skills`: the page, the commands, the settings; and below, the skills it carries.
pub const SKILLS: [(&str, &str); 4] = [
    ("commands/skills.toml", include_str!("../../../packs/skills/commands/skills.toml")),
    ("pages/skills.toml", include_str!("../../../packs/skills/pages/skills.toml")),
    ("settings/skills.toml", include_str!("../../../packs/skills/settings/skills.toml")),
    ("views/skills.toml", include_str!("../../../packs/skills/views/skills.toml")),
];

/// The files of the pack `reflexes`: the shipped reflexes (both off), their cases, `:reflex` and `:reflexes`, the page, the settings.
pub const REFLEXES: [(&str, &str); 6] = [
    ("needs/needs.toml", include_str!("../../../packs/reflexes/needs/needs.toml")),
    ("commands/reflexes.toml", include_str!("../../../packs/reflexes/commands/reflexes.toml")),
    ("pages/reflexes.toml", include_str!("../../../packs/reflexes/pages/reflexes.toml")),
    ("settings/reflexes.toml", include_str!("../../../packs/reflexes/settings/reflexes.toml")),
    ("reflexes/shipped.toml", include_str!("../../../packs/reflexes/reflexes/shipped.toml")),
    ("views/reflexes.toml", include_str!("../../../packs/reflexes/views/reflexes.toml")),
];

/// The skills the packs carry, `(pack, folder, SKILL.md)`: read when the pack is on, by name, in the order of the packs.
pub const SKILL_FILES: [(&str, &str, &str); 9] = [
    ("skills", "culprit-commit", include_str!("../../../packs/skills/skills/culprit-commit/SKILL.md")),
    ("skills", "debug-cause", include_str!("../../../packs/skills/skills/debug-cause/SKILL.md")),
    ("skills", "j4n-checks", include_str!("../../../packs/skills/skills/j4n-checks/SKILL.md")),
    ("skills", "j4n-docs", include_str!("../../../packs/skills/skills/j4n-docs/SKILL.md")),
    ("skills", "j4n-log", include_str!("../../../packs/skills/skills/j4n-log/SKILL.md")),
    ("skills", "j4n-pack", include_str!("../../../packs/skills/skills/j4n-pack/SKILL.md")),
    ("skills", "log-hunt", include_str!("../../../packs/skills/skills/log-hunt/SKILL.md")),
    ("skills", "narrow-reads", include_str!("../../../packs/skills/skills/narrow-reads/SKILL.md")),
    ("skills", "release-chores", include_str!("../../../packs/skills/skills/release-chores/SKILL.md")),
];

/// J4n basic, the packs that are on when the operator has said nothing: the gate and Log, the guard, the way to grow J4n, `connect` (each CLI agent that is installed
/// takes a chat turn and a task), and `lanes` and `crew`, which only the word `orchestrate` of `lanes` sets to work (under `single` they change nothing). The others wait.
pub const ON: [&str; 6] = ["base", "guard", "grow", "connect", "lanes", "crew"];

/// The harness of a start: `ON`, or every pack but `test` with `packs.full`, and the packs the operator's folders already use; the `enable` rows over
/// them; and why each pack that is on and not basic is on.
pub fn harness(catalog: &Catalog, folders: &[Layer], rows: &[j4n_core::pack::EnableRow]) -> (Harness, std::collections::BTreeMap<String, String>) {
    let (on, kept) = catalog.defaults(&ON, &["test"], folders);
    (catalog.harness(&on.iter().map(String::as_str).collect::<Vec<_>>(), rows), kept)
}

/// Why each pack that is on and is not J4n basic is on: `(id, the reason)`. A pack the folder already uses says what it has (`kept`), else an
/// `enable` row of the operator's, else `packs.full`, else it is on because another pack needs or extends it. The doctor says `on` with this.
pub fn why_on(harness: &Harness, kept: &std::collections::BTreeMap<String, String>, rows: &[j4n_core::pack::EnableRow], full: bool) -> Vec<(String, String)> {
    let enabled = |id: &str| rows.iter().any(|r| r.on && r.id == id);
    harness
        .info
        .iter()
        .filter(|i| i.on && !ON.contains(&i.id.as_str()))
        .map(|i| {
            let why = match kept.get(&i.id) {
                Some(w) => w.clone(),
                None if enabled(&i.id) => "an enable row turns it on".into(),
                None if full => "packs.full is on".into(),
                None => "another pack needs it".into(),
            };
            (i.id.clone(), why)
        })
        .collect()
}

macro_rules! manifest {
    ($id:literal) => {
        include_str!(concat!("../../../packs/", $id, "/pack.toml"))
    };
}

/// Every pack the binary carries, each with the manifest and the files it has on disk.
pub fn catalog() -> Catalog {
    let packs: [(&str, &[(&str, &str)]); 24] = [
        (manifest!("base"), &BASE),
        (manifest!("agent"), &AGENT),
        (manifest!("code"), &CODE),
        (manifest!("connect"), &CONNECT),
        (manifest!("jev"), &JEV),
        (manifest!("grow"), &GROW),
        (manifest!("lanes"), &LANES),
        (manifest!("crew"), &CREW),
        (manifest!("guard"), &GUARD),
        (manifest!("terminal"), &TERMINAL),
        (manifest!("accounts"), &ACCOUNTS),
        (manifest!("bench"), &BENCH),
        (manifest!("shape"), &SHAPE),
        (manifest!("qc"), &QC),
        (manifest!("contribute"), &CONTRIBUTE),
        (manifest!("inbox"), &INBOX),
        (manifest!("ledger"), &LEDGER),
        (manifest!("parts"), &PARTS),
        (manifest!("habits"), &HABITS),
        (manifest!("kit"), &KIT),
        (manifest!("offload"), &OFFLOAD),
        (manifest!("skills"), &SKILLS),
        (manifest!("reflexes"), &REFLEXES),
        (manifest!("test"), &[]),
    ];
    // the manifests are the repository's own and a test reads them: one that does not read is a bug, not a thing to carry on past
    Catalog::new(packs.iter().map(|(manifest, files)| Pack::builtin(manifest, files).unwrap_or_else(|e| panic!("a built-in pack.toml: {e}"))).collect())
}
