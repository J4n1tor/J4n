"""Cut a long text into pieces and give each piece's number, its size and its headings and first lines, to see the shape of it before reading any of it.

run(text, size=1000) -> [{"i": 0, "chars": 912, "head": "# Title\\nThe first line of the paragraph"}, ...]
The pieces themselves are j4n.chunks(text, size): the same `i` indexes them.
"""


def run(text, size=1000):
    pieces = j4n.chunks(text, size)
    return [{"i": i, "chars": len(p), "head": h} for i, (p, h) in enumerate(zip(pieces, j4n.heads(pieces)))]
