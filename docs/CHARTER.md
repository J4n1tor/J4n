# J4n — laws (charter)

Each law has a test; a law without a check is a wish. The tests are in `crates/j4n/tests/laws` unless a law names another.

1. **Eight words.** Table, Log, Task, Doer, Ask, Gate, Rule, View (+ value types, Pack). A new feature is
   a pack. Core grows only when no primitive fits; then a primitive grows one field/reducer/step kind
   with **two consumers** and a test. *Test*: size ratchet.
2. **One loop.** Queued/Running/Cooling/Waiting/Done/Stopped; nothing else ends a task; compaction only at
   settled boundaries. *Test*: the loop battery.
3. **One log.** Every record is an event; every ledger is a fold. *Test*: a source scan finds no append-
   mode file opens outside the Log.
4. **One table.** Every declaration is data with `api`; kinds are things; one writer, every write a
   Change. *Test*: every core kind's struct round-trips its TOML schema examples.
5. **One gate.** Every effect calls it once; Block > Ask > Allow; floors immovable; grants never loosen a
   Block. *Test*: each effect verb has a test that tries to loosen a Block.
6. **One ask.** Every decision is an Ask with a deterministic fallback; picks ∈ offered set; approvals
   never reach a model; J4n works keyless. *Test*: the suite runs keyless; an approval is never put to a
   machine answerer, and a question a machine can answer never reaches the operator.
7. **One connect.** Anything outside is a `[[connector]]`; new dialects are the only core change. *Test*:
   a fixture never-seen CLI (a script) runs through `jsonl-map` with no Rust change.
8. **One view layer.** The TUI draws only what the protocol sends and never reads files or calls the core
   directly; models get context only through Briefs. *Test*: `j4n-tui` depends only on `j4n-proto` + UI
   crates; a fixture pack adds a kind + view + command and the headless TUI shows them with zero code
   change.
9. **Core-enforced floors** (no pack can remove them): open-ask precedence, hard caps, Block floors,
   offered-set validation, boundary-only compaction, redaction, approvals never to model answerers, rule
   cause-depth/rate limits.
10. **No domain in core; the cut list stays cut.** *Test*: an absurd-domain pack works unchanged;
    a case-insensitive scan for the cut word over every file and file name of the repository (but `.git`, `target`, `.claude`) finds it
    only in the file the law names.
11. **TUI is the front door.** Every `[[command]]` is reachable from the palette and plain words; CLI verbs
    are for scripts/tests. No task of the operator needs a shell command or a file written by hand: each has a
    key in the tab that shows it. *Test*: every command resolves to a palette row; the no-shell walk.
12. **Proposals, not self-application.** Anything J4n makes for itself applies only when the operator accepts it.
13. **Proxy always.** Every network client is built by the one builder that honours `https_proxy` (for an account's
    calls, the account's proxy).
    *Test*: `crates/j4n/tests/laws/proxy.rs` — no HTTP client or raw socket named outside `crates/j4n-connect/src/net.rs`,
    no client crate outside `j4n-connect`; the builder reads `https_proxy`/`HTTPS_PROXY`/`no_proxy`;
    an account's proxy replaces it for that account's calls and children.
14. **Clean agent text.** No budget words and no negative capability claims in any model-bound template.
    *Test*: a source law over templates in packs.

## Proposed laws (not yet adopted)

These two are practice today but are not yet held by a law of their own.

15. **Outcome before structure.** A change to what runs for a line lands only with its offline scenario numbers and the
    turn-path budget; a pack's rule or ask point stays on every line's path only while measurements show it helps.
    *Test*: the turn-path ratchet and the offline scenarios run in `scripts/check.sh`.
16. **Proof before claims.** A task is judged by its Log, not its words: an empty reply is not an answer; a task with no
    answer and no sign of work is a failure; every shell effect's exit code reaches the model and the screen; a state
    claim ("saved", "N items", "tests pass") with no matching event is sent back once and then marked.
    *Test*: the offline scenarios for empty answers and for claims without events.
