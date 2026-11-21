# Writing a pack

A pack is how J4n is extended: **data, plus any executable it points at, and no plug-in code**. A pack is a folder. This guide is
for the person who writes one.

    j4n pack new mypack          # a template: a manifest, a phrase, a test with a golden, a README
    j4n pack check mypack        # the manifest, every kind file, the tests and the goldens
    j4n pack install mypack      # check first, then copy into ~/.j4n/packs/ (off until you turn it on)
    j4n pack install https://example.org/me/mypack.git     # the same, from a git repository

## Layout

    mypack/
      pack.toml            the manifest (below)
      phrases/hello.toml   kind files: any folder names, any number of files, each holds `api = 1` and `[[kind]]` entries
      kinds/mine.toml      a kind of your own, if you need one
      tests/hello.toml     your cases (`[[expect]]`); not read as kind files
      tests/golden/…       the text a case compares with
      README.md, scripts…  anything else: copied on install, never read by the loader

`pack.toml` holds exactly these keys; any other is refused:

```toml
api = 1                      # this J4n reads api 1
id = "mypack"                # letters, digits, `_ - .`; also the folder an installed pack lives in
version = "0.1.0"            # major.minor.patch
summary = "what it is for, in one line"
use_when = "what a line looks like that this pack serves"   # optional; plain words, read by the model that chooses features in `orchestrate`
needs = ["base"]             # packs that must be on, or this one is left out and says why
extends = []                 # packs this one builds on; loaded below it even when nobody turned them on
stability = "experimental"   # experimental | beta | stable (`stable` is given only by J4n's own list; shown `beta`)

[ready]                      # optional: what the pack switches on for itself when you enable it
default = true               # ready to go is on for a pack you enable (a pack that is in J4n basic never fires it by itself)
[ready.set]                  # one to seven settings that exist, each with a value its kind takes
lanes = "orchestrate"
```

With `[ready]` the pack's row on Grow › Packs has a **ready to go** switch (`g`). Enabling the pack with it on writes the settings in the same edit as the enable
(one `:undo` takes all of it back) and shows the card for anything the feature lacks; turning the switch off, or the pack off, takes back what it wrote unless you
changed a value since.

A kind file is `api = 1` and arrays of tables named for kinds. An entry's `id` is shared by **every** pack (the same `kind` and `id` in two
packs are one entry whose fields come from the higher layer), so give your entries ids that are yours: the pack's name is a good start.
The folders `tests`, `log`, `folds`, `blobs`, `packs`, `secrets`, `bin`, `skills`, `cache`, `tmp` and the like, and every name that starts with a dot,
are never read for kind files.

## Skills

A pack may carry skills: `skills/<name>/SKILL.md`, a folder the loader never reads for kind files. The file starts with
frontmatter (`name`, `when`, `cost` in tokens) and the words a model reads follow it:

```
---
name: my-checks
when: tests, lint, Cargo.toml, *.rs, kind:write
cost: 200
---
# Run my project's checks
…
```

`description` (optional) is one plain sentence that says what the skill is for and when to read it ("Read before a task that asks for a changelog line, a version bump or a tag"). A doer
that lists skills by their description (claude) is shown that sentence; without one it is shown `Use when:` and the `when` words, which reads as a keyword soup and is rarely opened.

`where` (optional) is a path that must be in the project's folder for the skill to be picked there: the skills J4n ships say
`where: crates/j4n-core/Cargo.toml` because they are about J4n's own repository; with no `where` a skill fits every project (`:skill` and the page list
every skill whatever the project). `when` is a list of words, files (`*.rs`, `Cargo.toml`, `docs/`) and `kind:<write|read|plan>`; J4n picks the few skills that fit a task by those and
gives only them to the doer (the Skills page and `:skill` list, read and switch them). A built-in pack's skills are carried in the binary; an installed
pack's are read from `~/.j4n/packs/<id>/skills/` while the pack is on. The words under the frontmatter are model-bound text: the law of clean text reads them.

## The kinds

The core declares 29 kinds (`packs/base/kinds/*.toml`, each with `doc`, the fields an entry may hold and examples that are checked when the
kind is read). The ones a pack most often fills:

| Kind | An entry is |
|---|---|
| `phrase` | words (`:hello`) that run one of the core's commands, or start a flow |
| `command` | a named thing the operator asks for: a core command, a `flow` it starts, a `page` it opens |
| `setting` | a named number or switch with a default |
| `rule` | when this happens and that holds, do this (the loop's reactions) |
| `gate`, `check`, `box` | what the Gate allows or blocks; how work is checked; a wall around a program. These **widen** what J4n may do, so turning on a pack that adds one asks twice |
| `connector`, `model` | a program J4n can start and the models it serves |
| `event`, `fold`, `view`, `page` | what is logged, how the Log is reduced, a query over it, the tab that draws it |
| `kind` | a kind of your own (closed once declared; a second declaration is refused) |

A pack can also declare kinds of its own and fill them: see `packs/bench` (kind `case`) or `packs/accounts` (kind `account`) for whole packs of
that shape. Layers merge from the built-in packs up through your packs, your own files and the project's files, and a higher layer wins field by field.

## Naming a script the pack ships: `{pack}`

A pack is a folder, and the folder may hold scripts (`bin/x`) that its connectors and checks run. A path written into a row would break
under another `J4N_HOME`, so a pack writes `{pack}`: in the files of a pack read from a folder (`$J4N_HOME/packs/<id>/`, or the folder
`j4n pack check` is given) every `{pack}` is that folder, in any value of any row:

```toml
[[connector]]
id = "prove"
is = "tool"
dialect = "text"
program = "{pack}/bin/prove"
argv = ["{pack}/bin/prove", "{target}"]
risk = "exec"
net = false          # no network for this tool where a box can cut it
```

- A built-in pack is in the binary and has no folder: `{pack}` is left as written there, so a built-in pack does not use it.
- The word is the pack's own. In the operator's files (`~/.j4n`, the project's folder of the operator) it means nothing, and the file
  that says it is refused whole with the way out (`{pack}` … this layer is not a pack): write the path of the script, or put the row in a
  pack. A line of a comment is not read.
- The pin of an installed pack is the hash of its files as they are on disk, `{pack}` unexpanded, so moving the home does not unpin it.
- `net = false` on a `connector` of a tool or on a `check` cuts the network of that program where the place is a `box` whose row has `no_net`
  words, whatever the setting `box.net` says. Left out, `box.net` decides; a place that is not a box cannot cut it.

## Flows: who does a step, and what the launch pinned

A step `do = "who:<role>"` elects a doer for the role (the ask point `who:<role>`, then the rank over what the Log says each doer has done).
**What the launch pinned binds that election**, so a run never costs a model the operator did not choose:

- `--doer D`: D does every `who:<role>` step it is ready for and good for (a connector with no `roles` is good for any). A step that leaves
  it out (`exclude`) or a role it is not good for is elected as before, and the Log's `flow.elected` says why: `--doer D cannot take `work`: …`.
- `--model M`: the doer that is elected runs on M **when it serves M** (it lists it: a word like `haiku` is read against that doer's own
  list). One that does not serve M keeps its own pick and the same line says so: `--model M is not one of D's models: it keeps its own`. A
  doer that lists no models serves none, except the one `--doer` named, which is given the word as typed.
- A step that names its own `model` (or its `do` is a connector, `flow:<id>` or `model:<id>`) is the later word and the pin does not touch it;
  the host elected for the pinned model does not go with a model of the step's.
- The pin is the launch's (`--doer`, `--model`); a thread moved by `:switch` is not read by a flow.

### A step in a `box` sees a path only if it is listed

A step with `place = "box"` runs behind a wall: the host is read-only, `/tmp` is a private empty folder, credentials and J4n's own home are empty. A
folder the goal names under `/tmp` (a source tree a test made there) is not in the wall, and a writer looking for it spends its calls on the hunt.
**List it in the step's `reads`**: an entry that starts with `/` is a path the step may read, bound read-only into the wall (over the private `/tmp`; a
credentials folder stays hidden whatever is listed); the other entries are kinds that other steps write, as before. When the goal names an absolute
path that exists and that the wall hides, and the step does not list it, the task's first `run.notice` says so: `the box hides /tmp/x; list it in the
step's `reads` to see it`.

### What a flow may spend, and how wide a `map` goes

A `--usd` cap (or a task's own `usd_soft`) **binds the whole tree** of the task that has it: what its steps, the items of a `map`, the flows it
runs and its branches spend counts toward it. Past the cap the task says `task.cap_reached` once, on itself, and the base rule `soft-cap` **parks** it: it
stops (`capped`, with its session, and the menu offers to carry on), and so does everything still going under it, which is logged `capped` too and
not `orphaned`. A flow therefore stops at its cap, not at one and a half times it: that factor is the kill for a run that goes on past the cap
before a rule can park it (a long turn), and for a tree whose rules are off. A step is started with the less of its own limit and what the tree has
left, and a tree that is past its cap starts nothing new (the flow ends with `the money of the task it belongs to has run out`).

**When the hard stop lands.** The hard stop (1.5 × the cap) is checked against what a doer has *reported*. A doer that reports each call
(the model doers, pi, opencode, acp) is stopped at it. The claude CLI prices a turn only when the turn ends, so a long turn could end far past it.
Its stream does say the tokens of each call as it goes, so the loop prices
them at what the last priced turn of the same doer on the same model cost per token and stops the task when that estimate and the reported spend pass
the hard stop (`task.cap_estimated`, then `hardcap`). A model with no priced turn in the Log yet (the first chat on opus) is priced from the newest
priced turn of another model of the same doer, by the ratio of their price words in `offload.prices` (`haiku=1 sonnet=3 opus=5`). **A doer with no
priced turn at all, or a model with no price word, has no price, and nothing is estimated** (no invented number): its first turn lands at the turn's
end, as a doer that says nothing mid-turn does (a program that prints one result).

- `usd = <dollars>` on a step is that step's own limit (`Limits::usd_soft`); the tree's remainder can only make it smaller.
- The items of a `map` run `flow.map_width` at a time (a setting, default 4, 1 to 16): each is a process of its own, so a machine with little
  memory says less; the flow lists at most 20 items.
- A model step that is refused by the provider's API or its policy (a safeguards flag, a content refusal) did not finish, and **a step that
  did not finish ends the flow**, with the reason, unless the step is `soft`. A `soft` step turns the refusal into an answer (its verdict is a
  fail and its answer is the reason) that the next step reads. So a last-resort step on a bigger model is `soft`, followed by a `branch` on its
  verdict (`on = "steps.last.out.verdict.pass"`), and the flow ends with a word of its own and not with the provider's refusal.

### A connector for a program with permission rules of its own: `[connector.guard]`

opencode in run mode answers its own permission asks by rejecting them, and with a bare `--auto` it allows everything (`bash -c`, `find -delete`).
A connector that says `[connector.guard]` (`packs/connect/connectors/opencode.toml` is the example) is given, for each task, a
permission config that J4n makes and puts in the variable the row names (`OPENCODE_CONFIG_CONTENT`: nothing is written, the operator's own config is not
touched, and it outranks the project's); `args = ["--auto"]` is added only with it and answers what the config leaves to ask. Everything is denied but what
J4n's gate calls safe: the readers, the read verbs of git (also after `--no-pager`, `-c core.pager=cat` and `-C <dir>`), the test runners; a credential
file is never read, the web is denied, and only the task's own folders may be reached. `--approval full` adds what `full_allow` says (the repository's own
writes, `mkdir`, a file an interpreter runs) and `full_deny` takes back `bash -c` and `sh -c` (which J4n unwraps and opencode cannot), `rm`, `curl`/`wget`, a push, a reset, a clean and `branch -D`. Full is parity with J4n's own gate at `--approval full` (inline `python -c` and `node -e` included), except those wrappers; the strict level has no inline code. The lists come from the gate where it has them
and from the row where only the connector can say; **the order is the meaning** (the last rule that matches wins, and each command of a compound line is
matched on its own), so every denial is written after the allow it narrows.

Two notes for the pack that writes flows and gates:

- A gate row on a command can match inside the line: `when = "lower(cmd) matches '*forge test*'"` (a `matches` pattern with `*` at both ends;
  `lower(...)` makes it case-blind). The rows of `packs/base/gates/paths.toml` do the same for a path.
- A `map` step is given its item by the template of its **`goal`** (`{item}`, `{item.field}`): `with` is not read by a `map`, so a
  step that wants the item in a field says it in the goal.

## Testing: `j4n pack check PATH`

`check` reads the folder the way an installed pack is read, with what it `needs` and `extends` beneath it, and says each problem as
`path:line: why`. It exits 0 when nothing is refused and 1 otherwise; a warning (for the author) does not fail the pack. It verifies:

1. **the manifest**: it reads, has only the keys above, `api` is 1, the id is an id, `version` is major.minor.patch, and every pack in
   `needs` and `extends` is built in or installed here;
2. **every kind file** against its kind: not TOML, a key the kind does not hold, a wrong type, a value outside an `enum`, a required field
   that no layer sets, an unknown kind, an id used twice in a layer, words of a `phrase` that do not open with a colon. A problem the packs
   below yours already have is theirs and is not said again;
3. **your cases**, the `[[expect]]` entries of `tests/*.toml`;
4. **your goldens**, the text a case compares with, in `tests/golden/`.

A `ref` to nothing (a command's `flow`, a page's `view`, an entry the pack's `needs` do not give) **fails** the pack, with the file, the line and
the row (`[[command]] go: …: no flow `nowhere``); the loader of J4n only warns, as the thing may come from another pack that is on.

A file in `tests/` with no `[[expect]]` is yours (data for your own scripts) and gets a warning that `check` does not run it.

A case is one of three shapes (every key is checked; a typo is refused):

```toml
api = 1

[[expect]]                   # an entry is there, and optionally has a field with this value
has = "phrase:mypack"        # kind:id; `absent = true` says it must not be there
field = "command"
is = "status"                # any TOML value

[[expect]]                   # a line is a command of the phrase table
say = ":mypack"
runs = "status"              # a core command; or `starts = "flow-id"` for a flow; leave both out to say it is no command
args = []                    # optional: what follows the words

[[expect]]                   # what the pack renders is the text of the golden
show = "entries"             # `entries`: a `kind id` line for each entry your files hold; or a kind your files fill: its entries as merged
golden = "entries.txt"       # tests/golden/entries.txt
```

`id = "…"` names a case in the messages. When a golden differs, the message names the golden's first differing line;
`J4N_BLESS=1 j4n pack check PATH` writes the goldens that differ, and you read the diff in git before you keep it.

## Installing: `j4n pack install PATH-OR-URL`

- It runs `check` first and **refuses a pack that fails**, with the same lines. Nothing is copied then.
- A **git URL** (`https://…`, `ssh://…`, `file://…`, `git@host:path`, or a name ending in `.git`) is cloned shallow (`--depth 1`) into a
  temporary folder, checked there, installed, and the clone is deleted. The pack is the repository's root: `pack.toml` must be at the top.
  The network is touched only by this command, once, and never by starting J4n.
- The pack is copied to `~/.j4n/packs/<id>/` (`$J4N_HOME/packs/<id>/`): its kind files, `pack.toml`, `tests/`, the README and your scripts
  (no hidden files, no links, 8 MB at most). A pack whose id is there already, or is a built-in pack's id, is not installed.
- **Installing is not turning on.** Turn it on in J4n with `:pack on mypack`: that is a proposal, and it pins the hash of the pack's
  `.toml` files, so a pack that changes afterwards is left out until you turn it on again. A pack that adds a `gate`, `check`, `box`,
  `rule`, `connector` or `model` asks twice. The pin covers every file of the installed folder, scripts, hidden names and links
  included: a script that changes afterwards leaves the pack out until you turn it on again, and a pack you turned on before
  that change and that holds a script asks once more.

A pack that is meant to be shared is a git repository whose top folder is the pack. Put `j4n pack check .` in its CI.
