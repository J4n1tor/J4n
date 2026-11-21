"""Call one tool once for each item of a list, or each line of a file, in one `j4n.many`, and count how many worked and how many failed.

run(tool, items, arg=None, template=None, fixed=None) -> {"ok": 9, "failed": 1, "results": [text or None, ...], "failures": [[item, why], ...]}
`items` is a list, or the path of a text file whose non-blank lines are the items. `arg` is the tool's argument that takes the item (default:
its only required one); `template` makes the argument from the item (`"wc -l {}"`); `fixed` holds the other arguments. A call the Gate holds
back or the tool could not do is a failure, and the rest still run.
"""


def run(tool, items, arg=None, template=None, fixed=None):
    if isinstance(items, str):
        with open(items, encoding="utf-8") as f:
            items = [line.strip() for line in f if line.strip()]
    items = list(items)
    if arg is None:
        schema = j4n.tools(tool)
        need = schema.get("required") or list(schema.get("properties", {}))
        if len(need) != 1:
            raise ValueError(f"each: {tool} takes {need or 'no arguments'}; say which one gets the item with arg=")
        arg = need[0]
    args = [{**(fixed or {}), arg: template.format(i) if template else i} for i in items]
    got = j4n.many(tool, args, errors="keep")
    failures = [[i, str(g)] for i, g in zip(items, got) if isinstance(g, Exception)]
    return {
        "ok": len(items) - len(failures),
        "failed": len(failures),
        "results": [None if isinstance(g, Exception) else g for g in got],
        "failures": failures,
    }
