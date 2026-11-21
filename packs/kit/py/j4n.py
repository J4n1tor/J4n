"""j4n, the kit's library: stdlib only, imported inside the `py` kernel as `import j4n`.

This is the code layer, the cheapest one: exact, free, instant. What the data fixes is done here; a typed judgment over text
(judge, choose, score) is asked of a decider (Jev) through the host calls, and new text of a chat model, last.

    chunks(text, n)        pieces of about n characters that follow the paragraph breaks
    table(md_or_csv)       the rows of a markdown or CSV table, as dicts of text
    grep(items, pattern)   the indexes of the items the pattern matches (exact, case as written)
    dedup(items)           the items without exact or whitespace-equal repeats, first one kept
    heads(items)           a short view of each item: its headings and the first line of each paragraph, made by code
    recipes()              the ready programs, by name, with the line that says what each does
    recipe(name, **kw)     run one of them

    judge(items, question)             {index: p} that the question holds for each item (a decider, in batches)
    choose(items, question, options)   {index: (option, p)}: the option a decider picks for each item
    score(items, question, levels)     {index: s}: a position from 0 to len(levels) - 1 for each item
    keep(p, at=None)                   the indexes with p >= at (the setting kit.keep, 0.15): dismiss as little as possible
    model(prompt, context=None, shape=None)   one prompt to a chat model, last, on what was kept
    tools(name=None)                   the tools code may call, as {name: what it does}; tools(name) is that tool's arguments
    call(name, **args)                 one of J4n's tools, as a model would call it: under the same rules, giving its text
    many(name, [args, ...], errors=)   the same tool over a list, at once, the texts in order
    fetch(url)                         a page, as the web tool reads it (a call of that tool)
    pick(items, question, view=heads)  EXPERIMENTAL: judge a cheap view of groups first, then the items of the groups that stay
    need(name, temp=False)             add a package (uv, asked first) and put it on sys.path; temp=True is for this task only
    host(name, **payload)              the closed set of calls to J4n: jev_many, model, need, tool, tools

judge, choose and score need the setting kit.judge on for the project; until then they raise an error that names it and
nothing leaves. Every call is checked, redacted and counted by J4n, not by this file: the kernel has no key and no network.

Code drops only by fact (rule 1): nothing here guesses at meaning. A function that cannot be exact says so.
"""
import csv
import functools
import importlib
import io
import json
import re
import sys

__all__ = ["chunks", "table", "grep", "dedup", "heads", "recipes", "recipe", "judge", "choose", "score", "keep", "pick", "model", "tools", "call", "many", "fetch", "host", "need", "HostError", "Refused"]

_FENCE = ("```", "~~~")
_HEAD = re.compile(r"^\s{0,3}#{1,6}\s+\S")
_RULE = re.compile(r"^[\s|:-]*-[\s|:-]*$")  # the line under a header: pipes, colons, dashes and spaces
_BLANK = re.compile(r"\n[ \t]*\n")
LINE = 160  # the longest line `heads` keeps whole


def _text(item):
    """The text of an item: a string as it is, a dict by its values, anything else by `str`."""
    if isinstance(item, str):
        return item
    if isinstance(item, dict):
        return " ".join(str(v) for v in item.values())
    return str(item)


# ---- chunks ---------------------------------------------------------------------------------------------------------------
def _cut(piece, n):
    """A paragraph longer than n, as pieces of at most n: at a line break if there is one, else at a space, else mid-word."""
    out = []
    while len(piece) > n:
        at = piece.rfind("\n", 0, n + 1)
        if at <= 0:
            at = piece.rfind(" ", 0, n + 1)
        if at <= 0:
            at = n
        out.append(piece[:at].rstrip())
        piece = piece[at:].lstrip("\n ")
    if piece.strip():
        out.append(piece)
    return out


def chunks(text, n=1000):
    """Cut `text` into pieces of at most `n` characters that follow its paragraph breaks.

    Paragraphs (blocks between blank lines) are packed whole, in order, into a piece until the next one would not fit; a paragraph
    longer than `n` is cut at a line break, else a space, else mid-word. No text is lost: joined with a blank line the pieces hold
    every word of the original in order. The pieces have no blank lines at their ends; an empty text gives []."""
    if not isinstance(text, str):
        raise TypeError("j4n.chunks needs a text")
    if n < 1:
        raise ValueError("j4n.chunks needs n of at least 1")
    out, cur = [], ""
    for para in _BLANK.split(text.strip("\n")):
        para = para.strip("\n")
        if not para.strip():
            continue
        for part in _cut(para, n):
            if cur and len(cur) + 2 + len(part) > n:
                out.append(cur)
                cur = ""
            cur = cur + "\n\n" + part if cur else part
    if cur:
        out.append(cur)
    return out


# ---- table ----------------------------------------------------------------------------------------------------------------
def _cells(line):
    """The cells of one markdown row: split at `|` that is not `\\|`, the outer pipes dropped, each cell stripped."""
    body = line.strip()
    parts = re.split(r"(?<!\\)\|", body)
    if body.startswith("|"):
        parts = parts[1:]
    if body.endswith("|") and not body.endswith("\\|"):
        parts = parts[:-1]
    return [p.replace("\\|", "|").strip() for p in parts]


def _names(header):
    """Column names: the header cells, an empty one named `_<position>`, a repeat given `_2`, `_3`."""
    seen, out = {}, []
    for i, h in enumerate(header):
        h = h or f"_{i + 1}"
        seen[h] = seen.get(h, 0) + 1
        out.append(h if seen[h] == 1 else f"{h}_{seen[h]}")
    return out


def _rows(header, rows):
    names = _names(header)
    out = []
    for r in rows:
        d = dict(zip(names, r + [""] * (len(names) - len(r))))
        for i in range(len(names), len(r)):  # a row longer than the header keeps its extra cells
            d[f"_{i + 1}"] = r[i]
        out.append(d)
    return out


def table(source):
    """The rows of a table in `source` as a list of dicts of text (nothing is turned into a number).

    A markdown table is any run of lines that start with `|`; the first one found is read, and text around it is ignored. With no
    such lines the source is read as CSV (the first line is the header). A short row is filled with empty text; a long one keeps its
    extra cells under `_<position>`. No table gives []."""
    if not isinstance(source, str):
        raise TypeError("j4n.table needs a text")
    lines = source.splitlines()
    run, started = [], False
    for ln in lines:
        if ln.lstrip().startswith("|"):
            run.append(ln)
            started = True
        elif started:
            break
    if run:
        if len(run) > 1 and _RULE.match(run[1]):  # only the line right under the header is the rule: a row of `-` cells is data
            del run[1]
        rows = [_cells(ln) for ln in run]
        return _rows(rows[0], rows[1:])
    rows = [r for r in csv.reader(io.StringIO(source)) if any(c.strip() for c in r)]
    return _rows([c.strip() for c in rows[0]], rows[1:]) if rows else []


# ---- grep, dedup ----------------------------------------------------------------------------------------------------------
def grep(items, pattern, flags=0):
    """The indexes of the items the regular expression `pattern` matches anywhere in (exact: case as written unless `flags` says
    `re.I`). A dict item is searched by its values."""
    rx = re.compile(pattern, flags)
    return [i for i, item in enumerate(items) if rx.search(_text(item))]


def _key(item):
    if isinstance(item, str):
        return " ".join(item.split())
    return json.dumps(item, sort_keys=True, default=str)


def dedup(items, index=False):
    """The items without a repeat of an earlier one, in their order. An item repeats an earlier one when it is the same text, or the
    same text apart from white space (runs of spaces, tabs and line breaks count as one space, the ends do not count). Case counts.
    With `index=True` the answer is the indexes of the items kept, so they can be used with the list the call was given."""
    seen, kept = set(), []
    for i, item in enumerate(items):
        k = _key(item)
        if k not in seen:
            seen.add(k)
            kept.append(i)
    return kept if index else [items[i] for i in kept]


# ---- heads ----------------------------------------------------------------------------------------------------------------
def _short(line):
    line = " ".join(line.split())
    return line if len(line) <= LINE else line[: LINE - 1].rstrip() + "…"


def _head_of(item):
    out, fenced = [], None
    for para in _BLANK.split(_text(item)):
        first = False  # the first line of this paragraph that is not a heading has been taken
        for ln in para.split("\n"):
            s = ln.strip()
            if s.startswith(_FENCE):
                fenced = None if fenced == s[:3] else (fenced or s[:3])
                continue
            if fenced or not s:
                continue
            if _HEAD.match(ln):
                out.append(_short(s))
            elif not first:
                first = True
                out.append(_short(s))
    return "\n".join(out)


def heads(items):
    """A short view of each item, made by code: its markdown headings, and the first line of each paragraph, in order, each line
    cut to 160 characters. Lines inside a code fence count as text, not as headings, and fence lines are left out. The answer has one
    string per item, so its indexes are the items' indexes."""
    return [_head_of(item) for item in items]


# ---- recipes --------------------------------------------------------------------------------------------------------------
_RECIPES = None  # {name: source}, set by the kernel when it loads this module; read from packs/kit/recipes/ when run from a file


def _sources():
    global _RECIPES
    if _RECIPES is None:
        import os

        base = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "recipes")
        _RECIPES = {}
        if os.path.isdir(base):
            for f in sorted(os.listdir(base)):
                if f.endswith(".py") and not f.startswith("_"):
                    with open(os.path.join(base, f), encoding="utf-8") as fh:
                        _RECIPES[f[:-3]] = fh.read()
    return _RECIPES


def _doc(source):
    m = re.match(r'\s*"""(.*?)"""', source, re.S)
    return " ".join(m.group(1).split("\n\n")[0].split()) if m else ""


def recipes():
    """The ready programs, as {name: what it does}. Run one with `recipe(name, ...)`."""
    return {name: _doc(src) for name, src in sorted(_sources().items()) if "recipe:" + name not in _OFF}


def recipe(name, **kw):
    """Run the recipe `name` with its keywords and give back what its `run` returns. `recipes()` names them; a recipe's own
    docstring says what it takes."""
    src = _sources().get(name)
    if src is None or ("recipe:" + name in _OFF and _depth == 1):
        raise KeyError(f"no recipe called {name!r}; there are: {', '.join(recipes()) or 'none'}")
    if _depth == 1:
        _USED["recipe:" + name] = _USED.get("recipe:" + name, 0) + 1
    ns = {"j4n": sys.modules.get("j4n") or sys.modules[__name__], "__name__": f"j4n.recipe.{name}"}
    exec(compile(src, f"<recipe {name}>", "exec"), ns)
    return ns["run"](**kw)


# ---- host calls -----------------------------------------------------------------------------------------------------------
CALLS = ("jev_many", "model", "need", "tool", "tools")  # the closed set J4n serves; any other name is refused here and there
PER_REQUEST = 24  # questions in one request (rule 4)
PER_CHARS = 24000  # characters of state in one request
_CONFIG = {}  # set by the kernel from J4n's load line: {"keep": 0.15}
_channel = None  # the kernel's line to J4n: _channel(name, payload) -> {"ok": ..., "value" | "error": ...}; None outside a kernel


class HostError(RuntimeError):
    """J4n refused a host call or it failed; the text says why and, when it can, which setting or item."""


def host(name, /, **payload):
    """Ask J4n for work and wait for the answer. `name` is one of CALLS; the payload is JSON. J4n checks the call (the Gate, the
    redactor, the project's setting kit.judge), does it, and counts what it cost. An error is a HostError, never a default."""
    if name not in CALLS:
        raise HostError(f"j4n.host has no call {name!r}; the calls are {', '.join(CALLS)}")
    if _channel is None:
        raise HostError("there is no kernel here to ask: host calls work inside the py tool")
    reply = _channel(name, payload)
    if not isinstance(reply, dict) or not reply.get("ok"):
        raise HostError(str((reply or {}).get("error") if isinstance(reply, dict) else reply) or f"{name} failed")
    return reply.get("value")


def _state(item):
    return item if isinstance(item, (str, dict, list)) else str(item)


def _size(item):
    return len(item) if isinstance(item, str) else len(json.dumps(item, default=str))


def _requests(items, idx, make):
    """One request per batch of at most PER_REQUEST items and PER_CHARS characters, in order. `make(i)` gives the question for item
    i; the state holds the batch's items by `p<i>`, so every answer is keyed by its item."""
    out, cur, size = [], [], 0
    for i in idx:
        n = _size(items[i])
        if cur and (len(cur) >= PER_REQUEST or size + n > PER_CHARS):
            out.append(cur)
            cur, size = [], 0
        cur.append(i)
        size += n
    if cur:
        out.append(cur)
    return [({"passages": {f"p{i}": _state(items[i]) for i in b}}, {f"p{i}": make(i) for i in b}, b) for b in out]


def _ask(items, idx, make):
    """Run the requests of one call at once (J4n runs up to 8 together) and give back {index: answer} for every item asked."""
    idx = list(range(len(items))) if idx is None else list(idx)
    reqs = _requests(items, idx, make)
    if not reqs:
        return {}
    replies = host("jev_many", calls=[{"state": s, "questions": q} for s, q, _ in reqs])
    if not isinstance(replies, list) or len(replies) != len(reqs):
        raise HostError("the decider's reply does not match the requests that were sent")
    out = {}
    for (_, _, batch), reply in zip(reqs, replies):
        if not isinstance(reply, dict) or reply.get("error") or not isinstance(reply.get("answers"), dict):
            raise HostError(f"the decider failed on items {batch[0]}..{batch[-1]}: {(reply or {}).get('error') if isinstance(reply, dict) else 'no reply'}")
        for i in batch:
            if f"p{i}" not in reply["answers"]:
                raise HostError(f"the decider gave no answer for item {i}")  # fail closed: a missing answer is not a default
            out[i] = reply["answers"][f"p{i}"]
    return out


def _p(a, i):
    p = a.get("noul") if isinstance(a, dict) and a.get("type", "noul") == "noul" else None
    if not isinstance(p, (int, float)) or isinstance(p, bool):
        raise HostError(f"the decider's answer for item {i} is not a probability: {a!r}"[:200])
    return min(1.0, max(0.0, float(p)))


def judge(items, question, idx=None):
    """{index: p}: how likely the question's answer is yes for each item (all of them, or those in `idx`). Items are packed into
    requests of at most 24 questions and 24,000 characters; the requests run at once. Needs kit.judge on for the project."""
    look = lambda i: {"type": "noul", "instructions": f"Look only at passage p{i} in the state. {question}"}
    return {i: _p(a, i) for i, a in _ask(items, idx, look).items()}


def _options(options):
    crit = dict(options) if isinstance(options, dict) else {str(o): str(o) for o in options}
    if len(crit) < 2:
        raise ValueError("j4n.choose needs at least two options")
    return crit


def choose(items, question, options, idx=None):
    """{index: (option, p)}: the option a decider picks for each item, and how sure it is. `options` is a list of words or a dict of
    word -> what it means. Needs kit.judge on for the project."""
    crit = _options(options)
    pick_one = lambda i: {"type": "choice", "instructions": f"Look only at passage p{i} in the state. {question}", "criteria": crit}
    out = {}
    for i, a in _ask(items, idx, pick_one).items():
        got = a.get("choice") if isinstance(a, dict) else None
        if got not in crit:
            raise HostError(f"the decider's pick for item {i} is not one of the options: {a!r}"[:200])
        p = a.get("confidence")
        if not isinstance(p, (int, float)) or isinstance(p, bool):
            p = (a.get("probabilities") or {}).get(got)
        out[i] = (got, min(1.0, max(0.0, float(p))) if isinstance(p, (int, float)) and not isinstance(p, bool) else None)
    return out


def score(items, question, levels, idx=None):
    """{index: s}: a position on the scale `levels` (words, lowest first), from 0 up to len(levels) - 1, for each item. Needs
    kit.judge on for the project."""
    levels = [str(x) for x in levels]
    if len(levels) < 2:
        raise ValueError("j4n.score needs at least two levels")
    rate = lambda i: {"type": "score", "instructions": f"Look only at passage p{i} in the state. {question}", "criteria": levels}
    out = {}
    for i, a in _ask(items, idx, rate).items():
        s = a.get("score") if isinstance(a, dict) else None
        if not isinstance(s, (int, float)) or isinstance(s, bool):
            raise HostError(f"the decider's score for item {i} is not a number: {a!r}"[:200])
        out[i] = min(float(len(levels) - 1), max(0.0, float(s)))
    return out


def keep(p, at=None):
    """The indexes of `p` (what judge gave) to hand on: everything the decider did not clearly rule out, p >= `at` (the setting
    kit.keep, 0.15 unless the project says otherwise). Dismiss as little as possible: the last reader is a chat model."""
    at = _CONFIG.get("keep", 0.15) if at is None else at
    return sorted(i for i, v in p.items() if v >= at)


_WARNED = False


def pick(items, question, view=None, group=8, at=None):
    """EXPERIMENTAL: {index: p} for the items of the groups that stay. Judges a cheap `view` of each group of `group`
    neighbours first (default `heads`, made by code), drops a group only when it is clearly ruled out (p < `at`), then judges the
    items of the groups that stay. Items of a dropped group are not in the answer. Whether the view keeps recall is not measured yet:
    `judge` is the default."""
    global _WARNED
    if not _WARNED:
        _WARNED = True
        print("j4n.pick is experimental: its recall is not measured; j4n.judge is the default", file=sys.stderr)
    at = _CONFIG.get("keep", 0.15) if at is None else at
    seen = list(view(items)) if callable(view) else list(view) if view is not None else heads(items)
    if len(seen) != len(items):
        raise ValueError("the view must have one entry for each item")
    groups = [list(range(k, min(k + group, len(items)))) for k in range(0, len(items), group)]
    blocks = ["\n\n".join(seen[i] for i in g) for g in groups]
    gp = judge(blocks, "Is there anything anywhere in this passage where: " + question)
    stay = [i for gi, g in enumerate(groups) if gp[gi] >= at for i in g]
    return judge(items, question, idx=stay)


def model(prompt, context=None, shape=None):
    """One prompt to a chat model, last, on what is left: `context` is a list of the texts it may use, `shape` a dict of the keys of a
    JSON answer (name -> what it holds). The answer is text, or with a shape a dict with those keys."""
    return host("model", prompt=prompt, context=[_state(c) for c in context] if context is not None else None, shape=shape)


# ---- the bridge: J4n's own tools from code, through one route; this block names no tool ------------
class Refused(HostError):
    """A call J4n did not make: the Gate blocks it or would ask first, a rule held it (a secret in the arguments, the cap of 200 calls a
    cell), or it is not a tool that code may call. The text says which."""


def tools(name=None):
    """The tools code may call, as {name: what it does}: the tools the model is offered, less the ones J4n keeps from code. `tools(name)`
    gives that tool's arguments (its JSON schema)."""
    listed = {t["name"]: t for t in host("tools")}
    if name is None:
        return {n: t["doc"] for n, t in listed.items()}
    if name not in listed:
        raise KeyError(f"no tool {name!r} for code; there are: {', '.join(listed) or 'none'}")
    return listed[name]["schema"]


def many(name, arglist, errors="raise"):
    """Call the tool `name` once per dict in `arglist`, at once (J4n runs up to 8 together; 200 calls a cell), and give the texts in
    order. Each call passes the Gate as a model's call would: one the Gate would ask about, or blocks, is a `Refused`, and one the tool
    could not do is a `HostError`. With errors="keep" a failed call gives its exception in its place and the rest still run."""
    if errors not in ("raise", "keep"):
        raise ValueError('j4n.many: errors is "raise" or "keep"')
    out = []
    for r in host("tool", tool=name, calls=[dict(a) for a in arglist]):
        e = Refused(r["text"]) if r.get("refused") else HostError(r["text"]) if r.get("error") else None
        if e is not None and errors == "raise":
            raise e
        out.append(r["text"] if e is None else e)
    return out


def call(name, /, **args):
    """Call J4n's tool `name` with its arguments, as a model's call: the text it gives back. `tools()` lists them."""
    return many(name, [args])[0]


# ---- end of the bridge ----------------------------------------------------------------------------------------------------------
def fetch(url):
    """The text of a page, as the web tool reads it: one call of that tool. A fetch the Gate would ask about is a `Refused`: ask for it with the tool yourself first."""
    return call("web", url=url)


def need(name, temp=False):
    """A package for this task: `need("pandas")` has J4n add it with `uv` and puts it on `sys.path`, so `import pandas` works in
    the next line. Name it as a string in the cell, alone or with others: J4n asks the operator before the cell runs, unless the package
    is granted or already there. `temp=True` is for this task only and goes with its kernel. A name only, no version. Returns a line that
    says what was done."""
    if not isinstance(name, str):
        raise TypeError("j4n.need takes a package name")
    reply = host("need", name=name, temp=bool(temp))
    if not isinstance(reply, dict) or not reply.get("path"):
        raise HostError("J4n gave no folder for the package")
    if reply["path"] not in sys.path:
        sys.path.insert(0, reply["path"])
    importlib.invalidate_caches()
    return reply.get("note") or f"{name} is ready"


# ---- use: which parts a cell used, and which the operator turned off ------------------------------------------------
# The kernel reports `_take_used()` with each answer and J4n folds it (`kit_use`). Only a call from a cell counts: one part calling another
# (`fetch` calls `call`, a recipe calls `many`) is the first part's use alone. A part named in the setting `kit.off` (the load line's
# `off`) is refused when a cell calls it and is left out of `help(j4n)`, `recipes()` and the card; nothing is deleted.
_PARTS = ("chunks", "table", "grep", "dedup", "heads", "recipes", "recipe", "judge", "choose", "score", "keep", "pick", "model", "tools", "call", "many", "fetch", "need")
_ALL = list(__all__)
_DOC0 = __doc__
_OFF = set()
_USED = {}
_depth = 0


def _counted(name, fn):
    @functools.wraps(fn)
    def part(*a, **kw):
        global _depth
        if _depth == 0:
            if name in _OFF:
                raise Refused(f"j4n.{name} is turned off in this project (the setting kit.off); the operator can turn it on again")
            _USED[name] = _USED.get(name, 0) + 1
        _depth += 1
        try:
            return fn(*a, **kw)
        finally:
            _depth -= 1

    return part


for _n in _PARTS:
    globals()[_n] = _counted(_n, globals()[_n])
del _n


def _take_used():
    """What the cell that just ran used, as {part: times}, and forget it."""
    global _depth
    _depth = 0
    out = dict(_USED)
    _USED.clear()
    return out


def _apply_off():
    """Read `_CONFIG["off"]` (part names; `recipe:each` for a recipe): refuse them, and leave them out of the docs and `__all__`."""
    global __all__, __doc__
    _OFF.clear()
    _OFF.update(str(n) for n in (_CONFIG.get("off") or ()))
    __all__ = [n for n in _ALL if n not in _OFF]
    gone = tuple(n + "(" for n in _OFF if ":" not in n) + tuple(n + " " for n in _OFF if ":" not in n)
    __doc__ = "\n".join(line for line in _DOC0.split("\n") if not line.startswith(tuple("    " + g for g in gone)))
