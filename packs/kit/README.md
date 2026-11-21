# The kit: code first, then a decider, then a model

This is the page to read before you write a cell.

The tool `py` runs Python in a kernel that keeps its variables for the task (standard library only, no network where the task is in a
box). It is on while the setting `kit.mode` is `on`. `import j4n` gives you the kit's library; nothing is installed.

An agent that has a shell of its own (claude, pi, codex) reaches the same kernel with `j4n py -c '<code>'` (or `-f file.py`): J4n opens the task's socket in `.j4n/run` while the agent runs, and a cell from the shell is one Gate call like any other. The Kit page lists the cells, and `:kit export` writes them as `<task>.ipynb`.

## When to use what

The step decides, not how hard it looks.

| The step needs | Use | Example | Why |
|---|---|---|---|
| an answer **fixed by the data**: parse, count, filter by a field, sort, join, dedup, exact match, arithmetic | **code** | rows whose `status` is `open`; lines matching a path; the sum of a column | exact, free, instant; a model here only adds error |
| a **typed judgment** over text that is given: yes/no, one of a closed set, a score on a scale | **a decider (Jev), called from code** (`j4n.judge`, `choose`, `score`; needs `kit.judge` on, see below) | is this passage about X; which of five areas is this bug in | about a second for a whole batch; typed answers that code can threshold and count |
| **new text**: a summary, an explanation, a plan, a patch | **a chat model, last**, on what code and the decider kept | write the report from the 12 kept pieces | the only step that needs generation, and it now reads a small part |

Four rules make the order hold:

1. **Code may drop only by fact.** A field, a type, an exact duplicate. Never a guess about meaning: a keyword search loses passages
   that say it in other words.
2. **A decider drops only what it clearly rules out** (p < 0.15 by default). The chat model is the last reader, so a kept false positive
   costs little and a dropped true one is lost for good.
3. **Write the program, let the program make the many calls.** Keep the data in variables and hand yourself a short result. Read the
   result, not the data: print counts and the kept items.
4. **Batch questions, not calls.** One call answers a whole list; a loop of single questions is the shape the kit does not offer.

## The code layer (built)

```python
import j4n

pieces = j4n.chunks(text, 1000)           # pieces of at most 1000 chars that follow the paragraph breaks
rows   = j4n.table(md_or_csv)             # the first markdown table, else CSV: a list of dicts of text
hits   = j4n.grep(pieces, r"TICKET-0[0-9]{2}") # indexes of exact matches (case counts; pass re.I to ignore it)
uniq   = j4n.dedup(pieces)                # drops exact and whitespace-equal repeats, keeps the first
view   = j4n.heads(pieces)                # per piece: its headings and the first line of each paragraph
```

- `chunks` loses no text and never goes past `n`; a paragraph longer than `n` is cut at a line, then a space, then anywhere.
- `table` keeps values as text (`"007"` stays `"007"`); a short row is filled with `""`, a long row keeps its extra cells as `_5`, `_6`.
- `grep` is a regular expression, searched in each item; a dict item is searched by its values.
- `dedup(items, index=True)` gives the indexes kept, so they line up with the list you gave.
- `heads` has one string per item, so `view[i]` is about `pieces[i]`. It is the cheap view a later pass can judge first.

## Recipes

Programs that worked, kept so that nobody writes them again. `j4n.recipes()` lists them; `j4n.recipe(name, ...)` runs one. Each one's
first lines say what it takes.

| Recipe | Does |
|---|---|
| `outline` | cut a text and show each piece's number, size, headings and first lines |
| `matching` | the pieces a pattern matches exactly, with their numbers |
| `near_duplicates` | groups of items that share almost all their words (reports; drops nothing) |
| `column_counts` | the most common values of each column of a table |
| `each` | one tool over a list or a file's lines in one `j4n.many`, with the ok and failed counts |
| `checks` | the check on every file the task changed, in one `each` |

A recipe of your own that is worth keeping becomes one by a Proposal, never by itself.

## The judge layer

```python
p    = j4n.judge(pieces, "Does this passage tell of a run killed for lack of RAM?")   # {index: p}
keep = j4n.keep(p)                                  # indexes with p >= kit.keep (0.15): only what is clearly ruled out is dropped
area = j4n.choose(rows, "Which area is this bug in?", ["tui", "gate", "jev"])      # {index: (option, p)}
rank = j4n.score(pieces, "How central is this?", ["aside", "related", "central"]) # {index: 0 .. len(levels) - 1}
text = j4n.model("Summarize what failed and why.", context=[pieces[i] for i in keep])   # last, on what was kept
page = j4n.fetch(url)                               # the web tool's policy; refused where the Gate would ask
```

- The kernel has no key and no network. `j4n.host(name, **payload)` writes a line to J4n and waits; J4n serves a closed set (`jev_many`,
  `model`, `need`, `tool`, `tools`), through the Gate's egress check, and counts what each call cost. Any other name is an error.
- `judge`, `choose` and `score` need the setting **`kit.judge`** `on` for the project (off by default): until then they raise a
  `HostError` that names it, and nothing is sent. With it on, a call that holds a line looking like a secret is refused whole, and the error
  says which items (`p17`), never the text. Drop or cut those and call again. `model` and `fetch` do not ask `kit.judge`; the egress Gate
  and the same leak check apply to them.
- Items are packed into requests of at most 24 questions and 24,000 characters; the requests run at once (8 together). Every answer is
  keyed by its item's index. A failed request, a missing answer or an odd one is an error, never a default.
- `pick(items, question, view=j4n.heads)` is **experimental**: it judges a code-made view of groups first. `judge` is the default.
- `need(package)` adds a Python package: `j4n.need("pandas")`, then `import pandas` in the next line. Name it as a **string in the cell**,
  one package a cell: J4n asks the operator **before the cell runs** (the install is an effect outside the task's folder), unless the package is
  already installed, in the setting `kit.grants` (`pandas numpy`), or granted for the task or session. It is `uv pip install --only-binary :all:`
  into J4n's own folder `~/.j4n/py/env` (all tasks share it; read-only in a box); `temp=True` installs for this task only and the folder goes with
  its kernel. A name only, no version. With no `uv` on the machine it says so, with the `pip --target` line for the operator to run.

## The bridge

The kernel has no tools of its own, and code can call the ones the model is offered, each call through the Gate as a model's call would be.

```python
j4n.tools()                      # {name: what it does}: the tools code may call; j4n.tools("read") gives one's arguments
text  = j4n.call("read", path="README.md")                   # one call, the text it gave back
texts = j4n.many("read", [{"path": p} for p in paths])       # up to 8 at once, 200 a cell; errors="keep" leaves a failed call as its exception
```

A call the Gate would ask about or block is a `Refused`; one the tool could not do is a `HostError`. The CLI doers get the same through `j4n py` and an offer line in
`prompts/card-cli.txt`.

## Turning a part off

The setting **`kit.off`** lists the library functions and recipes (`recipe:<name>`) that are off: a cell that calls one is refused and it is left out of `help(j4n)`, `j4n.recipes()`
and the offer line. Nothing is deleted. After 100 cells in at least 5 tasks with a part unused, the rule `rules/kit.toml` puts it to the operator as a Proposal; a yes writes the name
into `kit.off`.

## Files

| File | What |
|---|---|
| `py/kernel.py` | the kernel: JSON lines on stdin/stdout, one namespace per task |
| `py/j4n.py` | the library; J4n sends it to the kernel as its first line, so no file is in the box |
| `recipes/*.py` | the recipes; each defines `run(...)` and starts with a docstring |
| `prompts/card.txt` | what a model that has `py` is told |
| `tests/test_j4n.py` | the code layer's tests: `python3 -I -B packs/kit/tests/test_j4n.py` |
| `tests/test_judge.py` | the judge layer against a scripted host: `python3 -I -B packs/kit/tests/test_judge.py` |
| `tests/test_bridge.py` | the bridge (`tools`, `call`, `many`) against a scripted channel: `python3 -I -B packs/kit/tests/test_bridge.py` |
| `tests/test_recipes_bridge.py` | the recipes `each` and `checks` against a scripted channel: `python3 -I -B packs/kit/tests/test_recipes_bridge.py` |
| `tests/test_use.py` | what a cell used of the library and the parts turned off (`kit.off`): `python3 -I -B packs/kit/tests/test_use.py` |
| `prompts/card-cli.txt` | what a CLI doer that has a shell is told about `j4n py` |
| `rules/kit.toml` | the rule that proposes a part no cell uses for `kit.off` |
