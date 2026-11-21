"""The judge layer of the kit's library against a scripted host.

The host is a function here: no kernel, no J4n, no network. What J4n itself checks (the setting kit.judge, the leak check, the closed
set, the cost) is tested on the Rust side, with the real kernel; this file checks what the library asks and how it reads the answers.

Run: python3 -I -B packs/kit/tests/test_judge.py   (standard library only; scripts/check.sh runs it)
"""
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "py"))
import j4n  # noqa: E402


class Host:
    """A scripted J4n: answers jev_many from `p_of(item_index)`, and keeps what it was asked."""

    def __init__(self, p_of=lambda i: (i % 10) / 10, fail=None):
        self.calls, self.p_of, self.fail = [], p_of, fail

    def __call__(self, name, payload):
        self.calls.append((name, payload))
        if self.fail:
            return {"ok": False, "error": self.fail}
        if name != "jev_many":
            return {"ok": True, "value": f"{name}:{sorted(payload)}"}
        out = []
        for c in payload["calls"]:
            answers = {}
            for q, spec in c["questions"].items():
                i = int(q[1:])
                if spec["type"] == "noul":
                    answers[q] = {"type": "noul", "noul": self.p_of(i)}
                elif spec["type"] == "choice":
                    keys = sorted(spec["criteria"])
                    answers[q] = {"type": "choice", "choice": keys[i % len(keys)], "confidence": 0.5 + (i % 5) / 10}
                else:
                    answers[q] = {"type": "score", "score": i % 7}
            out.append({"answers": answers, "tokens": 9})
        return {"ok": True, "value": out}


def with_host(h):
    class Ctx:
        def __enter__(self):
            self.old = j4n._channel
            j4n._channel = h
            return h

        def __exit__(self, *a):
            j4n._channel = self.old

    return Ctx()


ITEMS = [f"passage {i}" for i in range(300)]


class Batching(unittest.TestCase):
    def test_s465_300_items_make_requests_of_at_most_24_and_come_back_keyed_by_item(self):
        with with_host(Host()) as h:
            p = j4n.judge(ITEMS, "Is it about X?")
        self.assertEqual(len(h.calls), 1, "all the requests of one judge go in one host call, which J4n runs at once")
        name, payload = h.calls[0]
        self.assertEqual(name, "jev_many")
        sizes = [len(c["questions"]) for c in payload["calls"]]
        self.assertEqual((sum(sizes), max(sizes), len(sizes)), (300, 24, 13))
        self.assertEqual(sorted(p), list(range(300)))
        self.assertTrue(all(p[i] == (i % 10) / 10 for i in range(300)), "each answer is for its own item")
        first = payload["calls"][0]
        self.assertEqual(sorted(first["state"]["passages"]), sorted(first["questions"]), "a request's state holds exactly the items it asks about")
        self.assertIn("passage p7", first["questions"]["p7"]["instructions"])
        self.assertEqual(first["state"]["passages"]["p7"], "passage 7")

    def test_a_request_is_also_cut_by_size(self):
        big = ["x" * 10000 for _ in range(7)]
        with with_host(Host()) as h:
            j4n.judge(big, "q")
        self.assertEqual([len(c["questions"]) for c in h.calls[0][1]["calls"]], [2, 2, 2, 1])

    def test_idx_asks_about_some_items_only_and_nothing_to_ask_calls_nothing(self):
        with with_host(Host()) as h:
            p = j4n.judge(ITEMS, "q", idx=[5, 250])
            self.assertEqual(sorted(p), [5, 250])
            self.assertEqual(j4n.judge([], "q"), {})
        self.assertEqual(len(h.calls), 1)

    def test_a_dict_item_is_sent_as_it_is(self):
        with with_host(Host()) as h:
            j4n.judge([{"id": 1, "note": "a"}], "q")
        self.assertEqual(h.calls[0][1]["calls"][0]["state"]["passages"]["p0"], {"id": 1, "note": "a"})


class Reading(unittest.TestCase):
    def test_a_probability_is_kept_between_0_and_1(self):
        with with_host(Host(p_of=lambda i: 1.7 if i else -0.2)):
            self.assertEqual(j4n.judge(["a", "b"], "q"), {0: 0.0, 1: 1.0})

    def test_choose_gives_the_option_and_how_sure(self):
        with with_host(Host()) as h:
            got = j4n.choose(["a", "b", "c"], "Which?", ["tui", "gate", "jev"])
        self.assertEqual(got[0], ("gate", 0.5))
        self.assertEqual(h.calls[0][1]["calls"][0]["questions"]["p1"]["criteria"], {"tui": "tui", "gate": "gate", "jev": "jev"})
        with with_host(Host()) as h:
            j4n.choose(["a"], "Which?", {"x": "the x way", "y": "the y way"})
        self.assertEqual(h.calls[0][1]["calls"][0]["questions"]["p0"]["criteria"], {"x": "the x way", "y": "the y way"})

    def test_choose_refuses_a_pick_that_was_not_offered(self):
        h = Host()
        h.p_of = None

        def odd(name, payload):
            return {"ok": True, "value": [{"answers": {"p0": {"type": "choice", "choice": "zzz", "confidence": 0.9}}}]}

        with with_host(odd), self.assertRaises(j4n.HostError) as e:
            j4n.choose(["a"], "q", ["x", "y"])
        self.assertIn("not one of the options", str(e.exception))

    def test_score_is_a_position_on_the_scale(self):
        with with_host(Host()):
            got = j4n.score(["a"] * 10, "How central?", ["aside", "related", "central"])
        self.assertEqual(got[1], 1.0)
        self.assertEqual(max(got.values()), 2.0, "a position is cut to the top level")

    def test_a_scale_or_a_choice_needs_two(self):
        with self.assertRaises(ValueError):
            j4n.score(["a"], "q", ["only"])
        with self.assertRaises(ValueError):
            j4n.choose(["a"], "q", ["only"])


class FailClosed(unittest.TestCase):
    def test_a_refusal_is_an_error_that_says_why_and_nothing_is_a_default(self):
        with with_host(Host(fail="the decider is off for this project: the setting kit.judge is off")):
            for call in (lambda: j4n.judge(["a"], "q"), lambda: j4n.choose(["a"], "q", ["x", "y"]), lambda: j4n.score(["a"], "q", ["lo", "hi"])):
                with self.assertRaises(j4n.HostError) as e:
                    call()
                self.assertIn("kit.judge", str(e.exception))

    def test_a_failed_request_is_an_error_naming_its_items(self):
        def half(name, payload):
            return {"ok": True, "value": [{"answers": {}, "error": "HTTP 503"} if k else {"answers": {q: {"type": "noul", "noul": 0.5} for q in c["questions"]}} for k, c in enumerate(payload["calls"])]}

        with with_host(half), self.assertRaises(j4n.HostError) as e:
            j4n.judge(ITEMS, "q")
        self.assertIn("items 24..47", str(e.exception))
        self.assertIn("HTTP 503", str(e.exception))

    def test_a_missing_or_odd_answer_is_an_error(self):
        for bad in ({}, {"p0": {"type": "noul"}}, {"p0": {"type": "noul", "noul": "high"}}, {"p0": {"type": "noul", "noul": True}}):
            with with_host(lambda n, p, bad=bad: {"ok": True, "value": [{"answers": bad}]}), self.assertRaises(j4n.HostError):
                j4n.judge(["a"], "q")

    def test_the_replies_must_match_the_requests(self):
        with with_host(lambda n, p: {"ok": True, "value": []}), self.assertRaises(j4n.HostError):
            j4n.judge(["a"], "q")


class Hosting(unittest.TestCase):
    def test_s467_host_names_only_the_closed_set(self):
        with with_host(Host()) as h:
            for name in ("shell", "jev", "", "JEV_MANY", "model "):
                with self.assertRaises(j4n.HostError) as e:
                    j4n.host(name)
                self.assertIn("jev_many, model, need, tool, tools", str(e.exception))
        self.assertEqual(h.calls, [], "an unknown name is refused before anything is written")

    def test_host_without_a_kernel_says_so(self):
        with self.assertRaises(j4n.HostError) as e:
            j4n.host("model", prompt="hi")
        self.assertIn("no kernel", str(e.exception))

    def test_model_passes_its_words_through(self):
        with with_host(Host()) as h:
            self.assertEqual(j4n.model("Say it.", context=["a", {"b": 1}], shape={"verdict": "yes or no"}), "model:['context', 'prompt', 'shape']")
        self.assertEqual(h.calls[0][1]["context"], ["a", {"b": 1}])


class Keeping(unittest.TestCase):
    def test_keep_dismisses_only_what_is_clearly_ruled_out(self):
        p = {0: 0.05, 1: 0.14999, 2: 0.15, 3: 0.42, 4: 0.9}
        self.assertEqual(j4n.keep(p), [2, 3, 4])
        self.assertEqual(j4n.keep(p, at=0.5), [4])
        self.assertEqual(j4n.keep(p, at=0.0), [0, 1, 2, 3, 4])

    def test_keep_reads_the_projects_line_from_the_kernel(self):
        old = dict(j4n._CONFIG)
        try:
            j4n._CONFIG["keep"] = 0.4
            self.assertEqual(j4n.keep({0: 0.3, 1: 0.4}), [1])
        finally:
            j4n._CONFIG.clear()
            j4n._CONFIG.update(old)


class Pick(unittest.TestCase):
    def test_pick_is_experimental_and_says_so_once(self):
        import contextlib
        import io

        j4n._WARNED = False
        err = io.StringIO()
        items = [f"# H{i}\n\nfirst {i}\nsecond {i}" for i in range(20)]
        with with_host(Host(p_of=lambda i: 0.9 if i == 1 else 0.0)) as h, contextlib.redirect_stderr(err):
            first = j4n.pick(items, "q")
            j4n.pick(items, "q")
        self.assertEqual(err.getvalue().count("experimental"), 1)
        self.assertIn("EXPERIMENTAL", j4n.pick.__doc__)
        # the coarse pass judged the code-made view of 3 groups; the second pass asked about the 8 items of the group that stayed
        coarse = h.calls[0][1]["calls"][0]["state"]["passages"]
        self.assertEqual(len(coarse), 3)
        self.assertIn("# H0", coarse["p0"])
        self.assertNotIn("second 0", coarse["p0"], "the view is the heads, not the text")
        self.assertEqual(sorted(first), list(range(8, 16)), "only the items of the group that stayed are in the answer")
        j4n._WARNED = True


if __name__ == "__main__":
    unittest.main(verbosity=1)
