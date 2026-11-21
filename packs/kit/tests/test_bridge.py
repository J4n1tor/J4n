"""The bridge of the kit's library against a scripted channel.

The channel is a function here: no kernel, no J4n, no tools. What J4n itself holds (the Gate per call, the taint, the leak walk, the
caps of 200 and 8) is tested on the Rust side with the real kernel (`bridge_cases`); this file checks what the library asks, how it reads
the answers, and that it knows no tool.

Run: python3 -I -B packs/kit/tests/test_bridge.py   (standard library only; scripts/check.sh runs it)
"""
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "py"))
import j4n  # noqa: E402

OFFER = [
    {"name": "echo", "doc": "Gives back its text.", "schema": {"type": "object", "properties": {"text": {"type": "string"}}}},
    {"name": "page", "doc": "A page.", "schema": {"type": "object"}},
]


class Channel:
    """A scripted J4n: `tools` gives OFFER; `tool` answers each call from `reply(tool, args)`, a text or a (text, kind) pair."""

    def __init__(self, reply=lambda tool, args: f"{tool}:{sorted(args.items())}", offer=None):
        self.seen, self.reply, self.offer = [], reply, offer or OFFER

    def __call__(self, name, payload):
        self.seen.append((name, payload))
        if name == "tools":
            return {"ok": True, "value": self.offer}
        if name != "tool":
            return {"ok": True, "value": None}
        rows = []
        for args in payload["calls"]:
            r = self.reply(payload["tool"], args)
            text, kind = r if isinstance(r, tuple) else (r, None)
            rows.append({"text": text, **({kind: True} if kind else {})})
        return {"ok": True, "value": rows}


def through(ch):
    class Ctx:
        def __enter__(self):
            self.old = j4n._channel
            j4n._channel = ch
            return ch

        def __exit__(self, *a):
            j4n._channel = self.old

    return Ctx()


class Listing(unittest.TestCase):
    def test_s473_tools_lists_what_the_host_offers_and_nothing_is_named_here(self):
        with through(Channel()) as ch:
            self.assertEqual(j4n.tools(), {"echo": "Gives back its text.", "page": "A page."})
            self.assertEqual(j4n.tools("echo"), OFFER[0]["schema"])
            with self.assertRaises(KeyError) as e:
                j4n.tools("py")
            self.assertIn("echo, page", str(e.exception))
        self.assertEqual([c[0] for c in ch.seen], ["tools"] * 3, "the list is asked for each time: a new tool is in it the moment the model has it")


class Calling(unittest.TestCase):
    def test_s474_call_is_one_tool_one_set_of_arguments_and_gives_the_text(self):
        with through(Channel()) as ch:
            self.assertEqual(j4n.call("echo", text="hi"), "echo:[('text', 'hi')]")
        self.assertEqual(ch.seen, [("tool", {"tool": "echo", "calls": [{"text": "hi"}]})])

    def test_a_tool_may_have_an_argument_called_name(self):
        with through(Channel()) as ch:
            j4n.call("echo", name="x")
        self.assertEqual(ch.seen[0][1]["calls"], [{"name": "x"}])

    def test_many_is_one_request_for_the_list_and_keeps_the_order(self):
        with through(Channel()) as ch:
            out = j4n.many("echo", ({"text": str(i)} for i in range(5)))
        self.assertEqual(out, [f"echo:[('text', '{i}')]" for i in range(5)])
        self.assertEqual(len(ch.seen), 1, "all in one round trip")
        with through(Channel()) as ch:
            self.assertEqual(j4n.many("echo", []), [])

    def test_s475_a_refused_call_is_a_refused_and_a_failed_one_a_host_error(self):
        script = lambda tool, args: ("the Gate asks before \"fetch a page\"", "refused") if args.get("n") == 1 else ("the tool failed", "error") if args.get("n") == 2 else "fine"
        with through(Channel(script)):
            with self.assertRaises(j4n.Refused) as e:
                j4n.call("page", n=1)
            self.assertIn("asks before", str(e.exception))
            self.assertTrue(issubclass(j4n.Refused, j4n.HostError))
            with self.assertRaises(j4n.HostError) as e:
                j4n.call("page", n=2)
            self.assertNotIsInstance(e.exception, j4n.Refused)
            with self.assertRaises(j4n.Refused):
                j4n.many("page", [{"n": 0}, {"n": 1}, {"n": 2}])
            kept = j4n.many("page", [{"n": 0}, {"n": 1}, {"n": 2}], errors="keep")
        self.assertEqual(kept[0], "fine")
        self.assertIsInstance(kept[1], j4n.Refused)
        self.assertIsInstance(kept[2], j4n.HostError)
        self.assertNotIsInstance(kept[2], j4n.Refused)

    def test_errors_is_raise_or_keep(self):
        with through(Channel()) as ch:
            with self.assertRaises(ValueError):
                j4n.many("echo", [{}], errors="ignore")
        self.assertEqual(ch.seen, [], "refused before anything was asked")

    def test_a_host_that_says_no_is_a_host_error(self):
        with through(lambda name, payload: {"ok": False, "error": "no tools are offered here"}):
            with self.assertRaises(j4n.HostError) as e:
                j4n.call("echo")
        self.assertIn("no tools are offered", str(e.exception))

    def test_fetch_is_call_web_by_another_name(self):
        with through(Channel()) as ch:
            self.assertEqual(j4n.fetch("https://example.org/"), "web:[('url', 'https://example.org/')]")
        self.assertEqual(ch.seen[0][1]["tool"], "web", "the alias is the one place the library names a tool")

    def test_the_closed_set_is_the_four_and_the_bridge(self):
        self.assertEqual(j4n.CALLS, ("jev_many", "model", "need", "tool", "tools"))
        with through(Channel()) as ch:
            with self.assertRaises(j4n.HostError):
                j4n.host("fetch", url="https://example.org/")
        self.assertEqual(ch.seen, [])


if __name__ == "__main__":
    unittest.main()
