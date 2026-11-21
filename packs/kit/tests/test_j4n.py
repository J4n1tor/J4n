"""The code layer of the kit's library on fixed fixtures gives fixed answers.

Run: python3 -I packs/kit/tests/test_j4n.py   (standard library only, no network, no file written; scripts/check.sh runs it)
"""
import os
import re
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "py"))
import j4n  # noqa: E402

DOC = """# Plan

The plan has two parts.

## Part one

First part, first line.
Second line of the first part.

A second paragraph here.

```
# not a heading
x = 1
```

## Part two

Last words."""

MD = """Some words before.

| id | status | note |
|----|:------:|------|
| 1 | open | a \\| b |
| 2 | done |
| 3 | open | x | extra |

After the table.

| other | table |
|---|---|
| ignored | ignored |
"""

CSV = 'name,count\nrust,3\n"py, thon",4\n\nzig\n'


class Chunks(unittest.TestCase):
    def test_paragraphs_are_packed_whole_and_in_order(self):
        text = "aaaa\n\nbbbb\n\ncccc\n\ndddd"
        self.assertEqual(j4n.chunks(text, 10), ["aaaa\n\nbbbb", "cccc\n\ndddd"])
        self.assertEqual(j4n.chunks(text, 4), ["aaaa", "bbbb", "cccc", "dddd"])
        self.assertEqual(j4n.chunks(text, 1000), [text])

    def test_a_long_paragraph_is_cut_at_a_line_then_a_space_then_anywhere(self):
        self.assertEqual(j4n.chunks("one two\nthree four\nfive", 10), ["one two", "three four", "five"])
        self.assertEqual(j4n.chunks("alpha beta gamma", 8), ["alpha", "beta", "gamma"])
        self.assertEqual(j4n.chunks("abcdefghij", 4), ["abcd", "efgh", "ij"])

    def test_no_text_is_lost_and_no_piece_is_longer_than_n(self):
        words = lambda s: "".join(s.split())
        for n in (7, 30, 100, 5000):
            pieces = j4n.chunks(DOC, n)
            self.assertEqual(words("\n\n".join(pieces)), words(DOC), n)
            self.assertTrue(all(len(p) <= n for p in pieces), n)

    def test_blank_lines_with_spaces_and_runs_of_them_are_one_break(self):
        self.assertEqual(j4n.chunks("a\n \t\n\n\n\nb", 1), ["a", "b"])

    def test_empty_and_bad_input(self):
        self.assertEqual(j4n.chunks("", 10), [])
        self.assertEqual(j4n.chunks("\n\n \n\n", 10), [])
        with self.assertRaises(ValueError):
            j4n.chunks("x", 0)
        with self.assertRaises(TypeError):
            j4n.chunks(["x"], 5)


class Table(unittest.TestCase):
    def test_the_first_markdown_table_is_read_and_the_text_around_it_is_not(self):
        self.assertEqual(
            j4n.table(MD),
            [
                {"id": "1", "status": "open", "note": "a | b"},
                {"id": "2", "status": "done", "note": ""},
                {"id": "3", "status": "open", "note": "x", "_4": "extra"},
            ],
        )

    def test_csv_when_there_are_no_pipes(self):
        self.assertEqual(j4n.table(CSV), [{"name": "rust", "count": "3"}, {"name": "py, thon", "count": "4"}, {"name": "zig", "count": ""}])

    def test_names_that_are_empty_or_repeat_are_made_distinct(self):
        self.assertEqual(j4n.table("| a | | a |\n|-|-|-|\n| 1 | 2 | 3 |"), [{"a": "1", "_2": "2", "a_2": "3"}])

    def test_values_stay_text(self):
        self.assertEqual(j4n.table("n\n007\n1.50"), [{"n": "007"}, {"n": "1.50"}])

    def test_no_table_is_no_rows(self):
        self.assertEqual(j4n.table(""), [])
        self.assertEqual(j4n.table("| only | header |\n|---|---|"), [])
        with self.assertRaises(TypeError):
            j4n.table(None)


class Grep(unittest.TestCase):
    ITEMS = ["TICKET-001 first", "nothing here", "ticket-002 lower", "see TICKET-003 and TICKET-004", {"id": "TICKET-005"}]

    def test_indexes_of_exact_matches(self):
        self.assertEqual(j4n.grep(self.ITEMS, r"TICKET-0\d\d"), [0, 3, 4])

    def test_case_counts_unless_the_flag_says_not(self):
        self.assertEqual(j4n.grep(self.ITEMS, r"ticket-0\d\d"), [2])
        self.assertEqual(j4n.grep(self.ITEMS, r"ticket-0\d\d", re.I), [0, 2, 3, 4])

    def test_no_match_is_empty_and_a_bad_pattern_is_an_error(self):
        self.assertEqual(j4n.grep(self.ITEMS, "zzz"), [])
        with self.assertRaises(re.error):
            j4n.grep(self.ITEMS, "(")


class Dedup(unittest.TestCase):
    ITEMS = ["a b", "a  b", "a\nb", "A b", "a b ", "c", "c", {"k": 1}, {"k": 1}]

    def test_exact_and_whitespace_equal_repeats_go_and_the_first_stays(self):
        self.assertEqual(j4n.dedup(self.ITEMS), ["a b", "A b", "c", {"k": 1}])

    def test_the_indexes_kept(self):
        self.assertEqual(j4n.dedup(self.ITEMS, index=True), [0, 3, 5, 7])

    def test_nothing_to_drop(self):
        self.assertEqual(j4n.dedup(["x", "y"]), ["x", "y"])
        self.assertEqual(j4n.dedup([]), [])


class Heads(unittest.TestCase):
    def test_headings_and_the_first_line_of_each_paragraph_in_order(self):
        self.assertEqual(
            j4n.heads([DOC]),
            ["# Plan\nThe plan has two parts.\n## Part one\nFirst part, first line.\nA second paragraph here.\n## Part two\nLast words."],
        )

    def test_one_view_for_each_item_and_a_fence_hides_its_headings(self):
        views = j4n.heads(["no heading\nsecond line", "", "```\n# comment\n```\nafter fence"])
        self.assertEqual(views, ["no heading", "", "after fence"])

    def test_a_heading_inside_a_paragraph_is_kept_beside_the_first_line(self):
        self.assertEqual(j4n.heads(["intro line\n## Inner heading\nbody line"]), ["intro line\n## Inner heading"])

    def test_a_long_line_is_cut(self):
        view = j4n.heads(["x" * 400])[0]
        self.assertEqual(len(view), j4n.LINE)
        self.assertTrue(view.endswith("…"))


class Recipes(unittest.TestCase):
    def test_the_list_names_each_recipe_with_its_line(self):
        r = j4n.recipes()
        self.assertEqual(sorted(r), ["checks", "column_counts", "each", "matching", "near_duplicates", "outline"])
        self.assertTrue(all(len(line) > 20 and "\n" not in line for line in r.values()), r)

    def test_outline(self):
        out = j4n.recipe("outline", text=DOC, size=70)
        self.assertEqual(
            out,
            [
                {"i": 0, "chars": 44, "head": "# Plan\nThe plan has two parts.\n## Part one"},
                {"i": 1, "chars": 54, "head": "First part, first line."},
                {"i": 2, "chars": 68, "head": "A second paragraph here.\n## Part two"},
                {"i": 3, "chars": 11, "head": "Last words."},
            ],
        )
        self.assertEqual([o["i"] for o in out], list(range(len(out))))
        self.assertEqual([o["chars"] for o in out], [len(p) for p in j4n.chunks(DOC, 70)])

    def test_matching(self):
        text = "alpha one\n\nbeta two\n\nalpha three"
        self.assertEqual(j4n.recipe("matching", text=text, pattern="alpha", size=12), [{"i": 0, "text": "alpha one"}, {"i": 2, "text": "alpha three"}])

    def test_near_duplicates(self):
        items = ["The cat sat on the mat", "the cat sat on the mat.", "dogs bark", "A different line", "The CAT sat on the mat!", "dogs bark loudly"]
        self.assertEqual(j4n.recipe("near_duplicates", items=items), [[0, 1, 4]])
        self.assertEqual(j4n.recipe("near_duplicates", items=items, at=0.5), [[0, 1, 4], [2, 5]])
        self.assertEqual(j4n.recipe("near_duplicates", items=["a b", "a b c d"], at=0.5), [[0, 1]])  # exactly at the line counts
        self.assertEqual(j4n.recipe("near_duplicates", items=["a b", "a b c d"], at=0.51), [])
        with self.assertRaises(ValueError):
            j4n.recipe("near_duplicates", items=["x"] * 2001)

    def test_column_counts(self):
        md = "| s | k |\n|--|--|\n| open | a |\n| done | a |\n| open | b |"
        self.assertEqual(j4n.recipe("column_counts", source=md), {"rows": 3, "columns": {"s": [["open", 2], ["done", 1]], "k": [["a", 2], ["b", 1]]}})
        self.assertEqual(j4n.recipe("column_counts", source=md, top=1)["columns"]["s"], [["open", 2]])

    def test_an_unknown_recipe_names_the_ones_there_are(self):
        with self.assertRaises(KeyError) as e:
            j4n.recipe("nope")
        self.assertIn("outline", str(e.exception))


class Need(unittest.TestCase):
    """`need` asks the host for a package and puts the folder J4n names on sys.path."""

    def setUp(self):
        self.was, self.path = j4n._channel, list(sys.path)

    def tearDown(self):
        j4n._channel = self.was
        sys.path[:] = self.path

    def test_need_outside_a_kernel_has_none_to_ask(self):
        j4n._channel = None
        with self.assertRaises(j4n.HostError) as e:
            j4n.need("pandas")
        self.assertIn("no kernel", str(e.exception))

    def test_the_folder_the_host_names_goes_on_the_path_once_and_the_note_comes_back(self):
        seen = []

        def channel(name, payload):
            seen.append((name, payload))
            return {"ok": True, "value": {"path": "/somewhere/py/env", "note": "pandas is ready"}}

        j4n._channel = channel
        self.assertEqual(j4n.need("pandas"), "pandas is ready")
        self.assertEqual(j4n.need("numpy", temp=True), "pandas is ready")
        self.assertEqual(seen, [("need", {"name": "pandas", "temp": False}), ("need", {"name": "numpy", "temp": True})])
        self.assertEqual(sys.path.count("/somewhere/py/env"), 1)
        self.assertEqual(sys.path[0], "/somewhere/py/env")

    def test_a_refusal_is_an_error_and_the_path_is_left_alone(self):
        j4n._channel = lambda name, payload: {"ok": False, "error": "was not asked about"}
        with self.assertRaises(j4n.HostError) as e:
            j4n.need("pandas")
        self.assertIn("not asked", str(e.exception))
        self.assertEqual(sys.path, self.path)

    def test_a_name_is_a_string(self):
        with self.assertRaises(TypeError):
            j4n.need(["pandas"])


if __name__ == "__main__":
    unittest.main(verbosity=1)
