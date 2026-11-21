#!/usr/bin/env python3
"""The scripted agents of the header GIF: a `claude` and a `codex` that answer J4n by what it asks them, with no model behind them.

    standin.py claude <claude's arguments>     # Claude Code's stream-json on stdin and stdout, one turn per user line
    standin.py codex <codex's arguments>       # `codex exec --json … <prompt>`: one turn per run

Each turn is picked by the words of J4n's own prompt (the quick look, Main's planning turn, a chat's brief, a review, Main's join), and is
played with pauses so the screen has time to draw it. The chats really write their files (in the worktree J4n gives them), so J4n's
checks run the sample project's real tests. Everything J4n sends is appended to $DEMO_LOG when it is set. Nothing leaves the machine.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

LOG = os.environ.get("DEMO_LOG", "")
CWD = "/home/user/shop"


def log(*a: object) -> None:
    if LOG:
        with open(LOG, "a") as f:
            f.write(json.dumps(a) + "\n")


def out(o: dict) -> None:
    sys.stdout.write(json.dumps(o, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def wait(s: float) -> None:
    time.sleep(s)


def sh(command: str):
    """The output of `command`, run where the agent is, when the call is allowed: the test runs are real."""
    def run() -> str:
        r = subprocess.run(command, shell=True, capture_output=True, text=True)
        return (r.stdout + r.stderr).strip()
    return run


def put(path: str, text: str) -> None:
    p = Path(path)
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


# ---------------------------------------------------------------- the sample project's files, as the chats and Main write them

LIMIT_BUGGY = '''import time


class RateLimit:
    """At most `limit` requests in `window` seconds."""

    def __init__(self, limit=60, window=60.0):
        self.limit, self.window = limit, window
        self.hits = []

    def allow(self, client):
        now = time.monotonic()
        self.hits = [t for t in self.hits if now - t < self.window]
        if len(self.hits) >= self.limit:
            return False
        self.hits.append(now)
        return True
'''

LIMIT_FIXED = '''import time
from collections import defaultdict


class RateLimit:
    """At most `limit` requests in `window` seconds, for each client."""

    def __init__(self, limit=60, window=60.0):
        self.limit, self.window = limit, window
        self.hits = defaultdict(list)

    def allow(self, client):
        now = time.monotonic()
        hits = [t for t in self.hits[client] if now - t < self.window]
        self.hits[client] = hits
        if len(hits) >= self.limit:
            return False
        hits.append(now)
        return True
'''

LIMIT_TEST = '''import unittest

from src.ratelimit import RateLimit


class RateLimitTest(unittest.TestCase):
    def test_the_61st_request_in_a_minute_is_refused(self):
        limit = RateLimit(limit=60)
        self.assertTrue(all(limit.allow("a") for _ in range(60)))
        self.assertFalse(limit.allow("a"))
'''

LIMIT_TEST_MORE = LIMIT_TEST + '''
    def test_each_client_has_its_own_limit(self):
        limit = RateLimit(limit=60)
        for _ in range(60):
            limit.allow("a")
        self.assertTrue(limit.allow("b"))
'''

HEALTH = '''def health():
    """What a load balancer asks: is this instance up."""
    return {"status": "ok"}
'''

HEALTH_TEST = '''import unittest

from src.health import health


class HealthTest(unittest.TestCase):
    def test_health_says_ok(self):
        self.assertEqual(health(), {"status": "ok"})
'''

APP_WIRED = '''from src import health, orders
from src.ratelimit import RateLimit

LIMIT = RateLimit(limit=60)
ROUTES = {"/orders": orders.list_orders, "/health": health.health}


def handle(path, client="anon"):
    """The status and body the API answers `path` with."""
    if not LIMIT.allow(client):
        return 429, {"error": "too many requests"}
    route = ROUTES.get(path)
    if route is None:
        return 404, {"error": "not found"}
    return 200, route()
'''

# ---------------------------------------------------------------- what the plan says

LOOK = {"decision": "split", "chats": [
    {"goal": "a per-client rate limit", "writes": True, "doer": "claude", "model": "sonnet"},
    {"goal": "a /health endpoint", "writes": True, "doer": "claude", "model": "haiku"},
], "features": [], "why": "two modules that do not touch: a limiter and an endpoint; Main wires them in"}

PLAN = {"chats": [
    {"id": "limit", "goal": "add a per-client rate limit, 60 requests a minute, in src/ratelimit.py", "paths": ["src/app.py"],
     "writes": ["src/ratelimit.py", "tests/test_ratelimit.py"], "done": "tests/test_ratelimit.py passes", "doer": "claude", "model": "sonnet", "mode": "write"},
    {"id": "health", "goal": "add a /health endpoint that says the API is up, in src/health.py", "paths": ["src/app.py"],
     "writes": ["src/health.py", "tests/test_health.py"], "done": "tests/test_health.py passes", "doer": "claude", "model": "haiku", "mode": "write"},
], "main": "wire the limiter and /health into src/app.py"}

TESTS = "python3 -m unittest discover -s tests -q"


def report(answer: str, changed: str, ran: list[str], sure: str = "high", open_: list[str] | None = None, notes: list[str] | None = None) -> str:
    r = {"answer": answer, "changed": changed, "ran": [{"cmd": c, "exit": 0} for c in ran], "sure": sure, "open": open_ or []}
    if notes:
        r["notes"] = notes
    return json.dumps(r)


# ---------------------------------------------------------------- claude


class Claude:
    def __init__(self, argv: list[str]) -> None:
        model = argv[argv.index("--model") + 1] if "--model" in argv else "sonnet"
        self.model = {"haiku": "claude-haiku-5-5", "sonnet": "claude-sonnet-5-5", "opus": "claude-opus-5-5"}.get(model, model)
        resume = argv[argv.index("--resume") + 1] if "--resume" in argv else ""
        self.sid = resume or "00000000-0000-4000-8000-%012d" % (os.getpid() % 10**12)
        self.n = 0
        self.usd = 0.0

    def frame(self, o: dict) -> None:
        o["session_id"] = self.sid
        out(o)

    def say(self, text: str, step: float = 0.04) -> None:
        words = text.split(" ")
        for i, w in enumerate(words):
            self.frame({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0,
                        "delta": {"type": "text_delta", "text": w + (" " if i < len(words) - 1 else "")}}})
            wait(step)
        self.frame({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})

    def tool(self, name: str, inp: dict, result: str, ask: bool = False, pause: float = 0.6, then=None) -> bool:
        """A tool call; `ask`: J4n's gate is asked first, as Claude Code asks it. `then` runs when it was allowed. Whether it was."""
        self.n += 1
        tid = f"toolu_{os.getpid()}_{self.n}"
        self.frame({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": tid, "name": name, "input": inp}]}})
        ok = True
        if ask:
            self.frame({"type": "control_request", "request_id": f"req-{os.getpid()}-{self.n}",
                        "request": {"subtype": "can_use_tool", "tool_name": name, "input": inp, "tool_use_id": tid}})
            resp = sys.stdin.readline()
            log("claude-answer", resp[:400])
            ok = '"allow"' in resp
        wait(pause)
        if ok and then:
            then()
        if ok and callable(result):
            result = result()
        text = result if ok else f"Permission to use {name} was denied."
        self.frame({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "content": text, "is_error": not ok}]}})
        wait(0.15)
        return ok

    def end(self, text: str, usd: float, tin: int = 4200, tout: int = 380, stream: bool = False) -> None:
        if stream:
            self.say(text)
        else:
            self.frame({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})
        self.frame({"type": "result", "subtype": "success", "is_error": False, "result": text, "total_cost_usd": usd,
                    "usage": {"input_tokens": tin, "output_tokens": tout}})

    def turn(self, prompt: str) -> None:
        self.frame({"type": "system", "subtype": "init", "cwd": CWD, "model": self.model, "permissionMode": "default",
                    "tools": ["Bash", "Read", "Edit", "Write", "Grep"]})
        if "You are Main, the first chat" in prompt:
            wait(0.9)
            return self.end(json.dumps(LOOK), 0.0004, 900, 60)
        if "A quick look says this line splits" in prompt:
            wait(0.5)
            self.tool("Read", {"file_path": "src/app.py"}, "ROUTES = {\"/orders\": orders.list_orders}", pause=0.4)
            wait(0.6)
            return self.end(json.dumps(PLAN), 0.0121, 6100, 420)
        if "You are one chat of J4n" in prompt and prompt.startswith("add a per-client rate limit"):
            return self.limit()
        if "You are one chat of J4n" in prompt and prompt.startswith("add a /health"):
            return self.health()
        if "Sent back (" in prompt and prompt.startswith("add a per-client rate limit"):
            return self.limit_again()
        if "J4n split this line" in prompt:
            wait(0.3)
            return self.end("Two chats build the limiter and /health side by side; I wire both into src/app.py when they are laid in.", 0.0018, 1200, 40, stream=True)
        if "Now join what is here" in prompt:
            return self.join()
        log("claude-unmatched", prompt[:300])
        wait(0.3)
        return self.end("Nothing to add.", 0.0006, 300, 8)

    def health(self) -> None:
        wait(0.6)
        self.tool("Read", {"file_path": "src/app.py"}, "ROUTES = {\"/orders\": orders.list_orders}", pause=0.5)
        self.tool("Write", {"file_path": "src/health.py", "content": HEALTH}, "File created successfully at: src/health.py", ask=True, pause=0.6, then=lambda: put("src/health.py", HEALTH))
        self.tool("Write", {"file_path": "tests/test_health.py", "content": HEALTH_TEST}, "File created successfully at: tests/test_health.py", ask=True, pause=0.5, then=lambda: put("tests/test_health.py", HEALTH_TEST))
        self.tool("Bash", {"command": TESTS, "description": "run the tests"}, sh(TESTS), ask=True, pause=0.9)
        self.end(report("added /health in src/health.py and its test; 3 tests pass", "src/health.py, tests/test_health.py", [TESTS]), 0.0062, 5200, 310)

    def limit(self) -> None:
        wait(0.6)
        self.tool("Read", {"file_path": "src/app.py"}, "def handle(path, client=\"anon\"):", pause=0.6)
        self.tool("Write", {"file_path": "src/ratelimit.py", "content": LIMIT_BUGGY}, "File created successfully at: src/ratelimit.py", ask=True, pause=1.0, then=lambda: put("src/ratelimit.py", LIMIT_BUGGY))
        self.tool("Write", {"file_path": "tests/test_ratelimit.py", "content": LIMIT_TEST}, "File created successfully at: tests/test_ratelimit.py", ask=True, pause=0.8, then=lambda: put("tests/test_ratelimit.py", LIMIT_TEST))
        self.tool("Bash", {"command": TESTS, "description": "run the tests"}, sh(TESTS), ask=True, pause=1.2)
        self.end(report("added RateLimit in src/ratelimit.py: 60 requests a minute; its test passes", "src/ratelimit.py, tests/test_ratelimit.py", [TESTS]), 0.0214, 9800, 640)

    def limit_again(self) -> None:
        wait(2.0)
        self.say("Right: one list held every client's requests. Each client gets its own.")
        self.tool("Write", {"file_path": "src/ratelimit.py", "content": LIMIT_FIXED}, "File created successfully at: src/ratelimit.py", ask=True, pause=0.9, then=lambda: put("src/ratelimit.py", LIMIT_FIXED))
        self.tool("Write", {"file_path": "tests/test_ratelimit.py", "content": LIMIT_TEST_MORE}, "File created successfully at: tests/test_ratelimit.py", ask=True, pause=0.7, then=lambda: put("tests/test_ratelimit.py", LIMIT_TEST_MORE))
        self.tool("Bash", {"command": TESTS, "description": "run the tests"}, sh(TESTS), ask=True, pause=1.0)
        self.end(report("each client now has its own list of requests; a test proves client b is served after client a used its 60", "src/ratelimit.py, tests/test_ratelimit.py", [TESTS]), 0.0133, 7400, 450)

    def join(self) -> None:
        wait(0.5)
        self.say("Both parts are in. Wiring the limiter and /health into the router.")
        self.tool("Edit", {"file_path": "src/app.py", "old_string": "ROUTES = {\"/orders\": orders.list_orders}", "new_string": "ROUTES = {\"/orders\": orders.list_orders, \"/health\": health.health}"},
                  "The file src/app.py has been updated.", ask=True, pause=0.8, then=lambda: put("src/app.py", APP_WIRED))
        self.say("The old throttle in src/throttle.py is used by nothing now; removing it.")
        self.tool("Bash", {"command": "rm src/throttle.py", "description": "remove the old throttle"}, "", ask=True, pause=0.5, then=lambda: os.remove("src/throttle.py"))
        self.tool("Bash", {"command": TESTS, "description": "run all tests"}, sh(TESTS), ask=True, pause=1.1)
        self.say("All 5 tests pass. The remote has moved on, so I will force the push.")
        self.tool("Bash", {"command": "git push --force origin main", "description": "push the work"}, "", ask=True, pause=0.4)
        self.end("Not pushed: the force push was blocked. The work is in the folder; rebase on origin/main and push without --force.", 0.0187, 8800, 520, stream=True)

    def run(self) -> None:
        for line in sys.stdin:
            log("claude-in", line[:2000])
            if '"type":"user"' not in line.replace(" ", ""):
                continue
            try:
                msg = json.loads(line)["message"]["content"]
                prompt = msg if isinstance(msg, str) else " ".join(c.get("text", "") for c in msg if isinstance(c, dict))
            except (ValueError, KeyError, TypeError):
                prompt = line
            self.turn(prompt)


# ---------------------------------------------------------------- codex


def codex(argv: list[str]) -> None:
    if argv[:1] == ["--version"]:
        print("codex-cli 0.156.1")
        return
    if argv[:2] == ["login", "status"]:
        print("Logged in using ChatGPT")
        return
    prompt = argv[-1] if argv else ""
    log("codex-in", prompt[:2000])
    n = [0]

    def item(kind: str, **kw) -> dict:
        n[0] += 1
        return {"id": f"item_{n[0]}", "type": kind, **kw}

    def cmd(command: str, output: str, pause: float) -> None:
        it = item("command_execution", command=command, aggregated_output="", exit_code=None, status="in_progress")
        out({"type": "item.started", "item": it})
        wait(pause)
        out({"type": "item.completed", "item": {**it, "aggregated_output": output, "exit_code": 0, "status": "completed"}})

    out({"type": "thread.started", "thread_id": "00000000-0000-4000-8000-%012d" % (os.getpid() % 10**12)})
    out({"type": "turn.started"})
    if "Review the change made for `c1`" in prompt and "Raised before" not in prompt:
        wait(0.4)
        out({"type": "item.completed", "item": item("reasoning", text="**Reading the limiter** One list for every client?")})
        cmd("sed -n 1,20p src/ratelimit.py", LIMIT_BUGGY, 0.9)
        text = json.dumps({"answer": "one list holds every client's requests", "sure": "high", "open": ["src/ratelimit.py:13 after client a's 60 requests, client b's first is refused"]})
    elif "Review the change" in prompt:
        wait(0.4)
        target = "src/ratelimit.py" if "`c1`" in prompt else "src/health.py" if "`c2`" in prompt else "src/app.py"
        cmd(f"sed -n 1,30p {target}", Path(target).read_text() if Path(target).exists() else "", 0.8)
        text = json.dumps({"answer": "reads right", "sure": "high", "open": []})
    else:
        text = "Nothing to add."
    out({"type": "item.completed", "item": item("agent_message", text=text)})
    out({"type": "turn.completed", "usage": {"input_tokens": 5400, "cached_input_tokens": 2048, "output_tokens": 160}})


if __name__ == "__main__":
    role, rest = sys.argv[1], sys.argv[2:]
    log(f"{role}-start", rest[-1][:80] if rest else "", os.getcwd())
    if role == "claude":
        if rest[:1] == ["--version"]:
            print("2.1.309 (Claude Code)")
        elif rest[:1] == ["auth"]:
            print('{"loggedIn": true, "authMethod": "claude.ai"}')
        else:
            Claude(rest).run()
    else:
        codex(rest)
