"""Report the items that say almost the same: groups of indexes whose word sets overlap by at least `at`. It reports and drops nothing; exact repeats are j4n.dedup.

run(items, at=0.9) -> [[0, 4], [2, 7, 9]]   (each group in order, the groups by their first index; an item alone is not listed)
Words are lower-cased runs of letters and digits; the overlap is shared words over all words either has. At most 2,000 items.
"""
import re

LIMIT = 2000


def run(items, at=0.9):
    if len(items) > LIMIT:
        raise ValueError(f"near_duplicates compares every pair, so it takes at most {LIMIT} items; cut the list first")
    sets = [frozenset(re.findall(r"[a-z0-9]+", j4n._text(t).lower())) for t in items]
    parent = list(range(len(items)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for i in range(len(items)):
        for j in range(i + 1, len(items)):
            a, b = sets[i], sets[j]
            if a or b:
                if len(a & b) / len(a | b) >= at:
                    parent[find(j)] = find(i)
    groups = {}
    for i in range(len(items)):
        groups.setdefault(find(i), []).append(i)
    return sorted(g for g in groups.values() if len(g) > 1)
