"""Find the pieces of a long text that a pattern matches exactly, each with its number: the part of a text that words can find without a judge.

run(text, pattern, size=1000, flags=0) -> [{"i": 3, "text": "..."}, ...]
`i` is the index in j4n.chunks(text, size). A pattern finds only what it spells: a passage that says the same in other words is not found.
"""


def run(text, pattern, size=1000, flags=0):
    pieces = j4n.chunks(text, size)
    return [{"i": i, "text": pieces[i]} for i in j4n.grep(pieces, pattern, flags)]
