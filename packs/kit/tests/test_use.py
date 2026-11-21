"""What a cell used of the library, and the parts the operator turned off.

The kernel reports `_take_used()` with each answer and J4n folds it; a part named in the load line's `off` is refused, hidden from `help`,
`__all__` and `recipes()`, and nothing is deleted. Here the library runs without a kernel, against a scripted channel.

Run: python3 -I -B packs/kit/tests/test_use.py   (standard library only; scripts/check.sh runs it)
"""
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "py"))
import j4n  # noqa: E402


def channel(name, payload):
    if name == "tools":
        return {"ok": True, "value": [{"name": "echo", "doc": "Echo.", "schema": {"type": "object", "properties": {"text": {"type": "string"}}}}]}
    return {"ok": True, "value": [{"text": "x"} for _ in payload.get("calls", [])]}


class Use(unittest.TestCase):
    def setUp(self):
        j4n._channel = channel
        j4n._CONFIG.clear()
        j4n._apply_off()
        j4n._take_used()

    def tearDown(self):
        j4n._CONFIG.clear()
        j4n._apply_off()
        j4n._take_used()
        j4n._channel = None

    def test_each_call_from_a_cell_counts_and_taking_forgets(self):
        j4n.chunks("a\n\nb", 1)
        j4n.chunks("c", 5)
        j4n.grep(["a", "b"], "a")
        self.assertEqual(j4n._take_used(), {"chunks": 2, "grep": 1})
        self.assertEqual(j4n._take_used(), {})

    def test_one_part_calling_another_is_the_first_parts_use_alone(self):
        j4n.fetch("http://x")
        self.assertEqual(j4n._take_used(), {"fetch": 1})
        j4n.many("echo", [{"text": "a"}, {"text": "b"}])
        self.assertEqual(j4n._take_used(), {"many": 1})

    def test_a_recipe_counts_as_itself_and_as_recipe_and_what_it_calls_does_not(self):
        j4n.recipe("each", tool="echo", items=["a", "b"], arg="text")
        self.assertEqual(j4n._take_used(), {"recipe": 1, "recipe:each": 1})

    def test_a_cell_that_fails_inside_a_part_leaves_the_depth_at_zero(self):
        with self.assertRaises(TypeError):
            j4n.chunks(5)
        j4n.table("a,b\n1,2")
        self.assertEqual(j4n._take_used(), {"chunks": 1, "table": 1})

    def test_the_parts_are_the_library_less_its_errors_and_its_host_line(self):
        self.assertEqual(set(j4n._PARTS), set(j4n._ALL) - {"host", "HostError", "Refused"})
        for n in j4n._PARTS:
            self.assertTrue(callable(getattr(j4n, n)), n)

    def test_a_part_that_is_off_is_refused_hidden_and_not_deleted(self):
        j4n._CONFIG["off"] = ["dedup", "heads", "recipe:each"]
        j4n._apply_off()
        with self.assertRaises(j4n.Refused) as e:
            j4n.dedup(["a"])
        self.assertIn("kit.off", str(e.exception))
        self.assertEqual(j4n._take_used(), {}, "a refused call is not a use")
        for gone in ("dedup", "heads"):
            self.assertNotIn(gone, j4n.__all__)
            self.assertNotIn("    " + gone + "(", j4n.__doc__)
        self.assertIn("    grep(", j4n.__doc__)
        self.assertNotIn("each", j4n.recipes())
        self.assertIn("outline", j4n.recipes())
        with self.assertRaises(KeyError):
            j4n.recipe("each", tool="echo", items=[], arg="text")
        # and on again: nothing was deleted
        j4n._CONFIG["off"] = []
        j4n._apply_off()
        self.assertEqual(j4n.dedup(["a", "a"]), ["a"])
        self.assertIn("dedup", j4n.__all__)
        self.assertIn("    dedup(", j4n.__doc__)

    def test_a_recipe_that_is_built_on_an_off_recipe_still_runs_it(self):
        # `checks` runs `each` inside itself: off hides `each` from the cell, not from the recipe that is built on it
        j4n._sources()
        j4n._RECIPES["wrap"] = 'def run():\n    return j4n.recipe("each", tool="echo", items=["a"], arg="text")\n'
        try:
            j4n._CONFIG["off"] = ["recipe:each"]
            j4n._apply_off()
            self.assertEqual(j4n.recipe("wrap")["ok"], 1)
            self.assertEqual(j4n._take_used(), {"recipe": 1, "recipe:wrap": 1})
        finally:
            del j4n._RECIPES["wrap"]


if __name__ == "__main__":
    unittest.main()
