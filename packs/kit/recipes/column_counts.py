"""Count how often each value turns up in each column of a markdown or CSV table: the first look at rows before filtering them.

run(source, top=5) -> {"rows": 12, "columns": {"status": [["open", 7], ["done", 5]], ...}}
Each column lists its `top` most common values, the most common first and ties in the order they first appear.
"""


def run(source, top=5):
    rows = j4n.table(source)
    names = list(dict.fromkeys(k for r in rows for k in r))
    columns = {}
    for name in names:
        count = {}
        for r in rows:
            v = r.get(name, "")
            count[v] = count.get(v, 0) + 1
        columns[name] = [[v, n] for v, n in sorted(count.items(), key=lambda kv: -kv[1])[:top]]
    return {"rows": len(rows), "columns": columns}
