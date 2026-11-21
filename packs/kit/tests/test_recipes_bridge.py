"""The recipes that call tools through the bridge, `each` and `checks`, against a scripted channel.

The channel is a function: no kernel, no J4n, no tools. What J4n holds (the Gate per call, the caps) is tested on the Rust side with the real
kernel (`bridge_cases`); this file checks what the recipes ask, in how many round trips, and how they count.

Run: python3 -I -B packs/kit/tests/test_recipes_bridge.py   (standard library only; scripts/check.sh runs it)
"""
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "py"))
sys.path.insert(0, HERE)
import j4n  # noqa: E402
from test_bridge import Channel, through  # noqa: E402


def listing(n):
    d = tempfile.mkdtemp()
    path = os.path.join(d, "items.txt")
    with open(path, "w", encoding="utf-8") as f:
        f.write("".join(f"item-{i}\n" + ("\n" if i % 4 == 0 else "") for i in range(n)))
    return path


class Each(unittest.TestCase):
    def test_s481_each_over_a_file_of_10_lines_makes_10_calls_in_one_many_and_counts_ok_and_failed(self):
        bad = lambda tool, args: ("the tool failed", "error") if args["text"] in ("item-3", "item-7") else ("the Gate asks", "refused") if args["text"] == "item-9" else f"{tool}:{args['text']}"
        with through(Channel(bad)) as ch:
            r = j4n.recipe("each", tool="echo", items=listing(10))
        calls = [c for c in ch.seen if c[0] == "tool"]
        self.assertEqual(len(calls), 1, "one many, one round trip")
        self.assertEqual(calls[0][1]["tool"], "echo")
        self.assertEqual([a["text"] for a in calls[0][1]["calls"]], [f"item-{i}" for i in range(10)], "the lines, in order, the blank ones left out")
        self.assertEqual((r["ok"], r["failed"]), (7, 3))
        self.assertEqual([f[0] for f in r["failures"]], ["item-3", "item-7", "item-9"])
        self.assertIn("the Gate asks", r["failures"][2][1])
        self.assertEqual(r["results"][0], "echo:item-0")
        self.assertIsNone(r["results"][3])

    def test_the_argument_is_the_tools_only_required_one_or_said_and_a_template_makes_it_from_the_item(self):
        with through(Channel()) as ch:
            j4n.recipe("each", tool="echo", items=["a", "b"], arg="text", fixed={"n": 1}, template="<{}>")
            with self.assertRaises(ValueError) as e:
                j4n.recipe("each", tool="page", items=["a"])
        self.assertEqual(ch.seen[0][1]["calls"], [{"n": 1, "text": "<a>"}, {"n": 1, "text": "<b>"}])
        self.assertIn("arg=", str(e.exception))

    def test_j113_the_example_the_offer_line_prints_runs_as_written_for_a_tool_with_one_required_argument(self):
        # the shapes of J4n's real tools: `read` lists three arguments and requires `path`; `run` requires `cmd`
        offer = [
            {"name": "read", "doc": "Read a text file.", "schema": {"type": "object", "properties": {"path": {}, "offset": {}, "limit": {}}, "required": ["path"]}},
            {"name": "run", "doc": "Run a shell command.", "schema": {"type": "object", "properties": {"cmd": {}, "timeout": {}}, "required": ["cmd"]}},
            {"name": "web", "doc": "A page or a search.", "schema": {"type": "object", "properties": {"q": {}, "url": {}}}},
        ]
        items = listing(10)
        with through(Channel(offer=offer)) as ch:
            r = j4n.recipe("each", tool="read", items=items)
            self.assertEqual((r["ok"], r["failed"]), (10, 0))
            self.assertEqual(list(ch.seen[-1][1]["calls"][0]), ["path"], "the one required argument gets the item")
            r = j4n.recipe("each", tool="run", items=items)
            self.assertEqual(list(ch.seen[-1][1]["calls"][0]), ["cmd"])
            # a tool with two arguments and none required is the one the line must not print `each` for: it asks for arg=
            with self.assertRaises(ValueError):
                j4n.recipe("each", tool="web", items=items)
            r = j4n.recipe("each", tool="web", items=items, arg="url")
            self.assertEqual(r["ok"], 10)


class Checks(unittest.TestCase):
    def test_checks_runs_the_check_on_each_changed_file_quoted_and_names_the_failures(self):
        status = "## main...origin/main\n M src/a.py\n?? src/new one.py\nR  old.py -> src/b.py\n?? build/\n"

        def reply(tool, args):
            if tool == "git":
                return status
            return ("exit 1\nboom", "error") if "b.py" in args["cmd"] else "ok"

        with through(Channel(reply)) as ch:
            r = j4n.recipe("checks", cmd="python3 -m py_compile {}")
        runs = [c for c in ch.seen if c[0] == "tool" and c[1]["tool"] == "run"]
        self.assertEqual(len(runs), 1, "all the files in one many")
        self.assertEqual([c["cmd"] for c in runs[0][1]["calls"]], ["python3 -m py_compile src/a.py", "python3 -m py_compile 'src/new one.py'", "python3 -m py_compile src/b.py"])
        self.assertEqual(r["files"], ["src/a.py", "src/new one.py", "src/b.py"])
        self.assertEqual((r["ok"], r["failed"]), (2, 1))
        self.assertEqual(r["failures"][0][0], "src/b.py")

    def test_checks_with_files_given_asks_no_status_and_no_files_is_no_call(self):
        with through(Channel()) as ch:
            r = j4n.recipe("checks", cmd="true {}", files=["x.py"])
            none = j4n.recipe("checks", cmd="true {}", files=[])
        self.assertEqual([c[1]["tool"] for c in ch.seen if c[0] == "tool"], ["run"])
        self.assertEqual((r["ok"], none["ok"], none["files"]), (1, 0, []))


class Listed(unittest.TestCase):
    def test_the_recipes_are_listed_with_what_they_do(self):
        names = j4n.recipes()
        self.assertIn("each", names)
        self.assertIn("checks", names)
        self.assertTrue(names["each"].startswith("Call one tool once for each item"), names["each"])


if __name__ == "__main__":
    unittest.main()
