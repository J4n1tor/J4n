#!/usr/bin/env python3
"""Write the header GIF's scripted Claude turn (`scenario.jsonl` beside this file): a failing test read, searched, fixed
and run again, then a force push that the gate stops. The frames have the shape of Claude Code's stream; `# wait` lines
pace them for the player."""

import json
from pathlib import Path

SID = "00000000-0000-4000-8000-00000000d3m0"
out: list[str] = []
n = 0


def frame(obj: dict) -> None:
    obj["session_id"] = SID
    out.append(json.dumps(obj, separators=(",", ":")))


def wait(s: float) -> None:
    out.append(f"# wait {s}")


def say(text: str, step: float = 0.05) -> None:
    words = text.split(" ")
    for i, w in enumerate(words):
        frame({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0,
               "delta": {"type": "text_delta", "text": w + (" " if i < len(words) - 1 else "")}}})
        wait(step)
    frame({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}})


def tool(name: str, inp: dict, result: str, ask: bool = False, error: bool = False, pause: float = 0.9) -> None:
    global n
    n += 1
    tid = f"toolu_{n}"
    frame({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": tid, "name": name, "input": inp}]}})
    if ask:
        frame({"type": "control_request", "request_id": f"req-{n}",
               "request": {"subtype": "can_use_tool", "tool_name": name, "input": inp, "tool_use_id": tid}})
    wait(round(pause * 0.7, 2))
    frame({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid, "content": result, "is_error": error}]}})
    wait(0.2)


out.append("# the header GIF's turn: a scripted session in the shape of Claude Code's stream, no model behind it")
frame({"type": "system", "subtype": "init", "cwd": "/home/user/shop", "model": "claude-sonnet-5-5", "permissionMode": "default",
       "tools": ["Bash", "Read", "Edit", "Grep"]})
wait(0.6)
say("Let me run the failing test first.")
tool("Bash", {"command": "pytest -q tests/test_login.py", "description": "run the login tests"},
     "F..\nFAILED tests/test_login.py::test_token_expires - assert False\n1 failed, 2 passed in 0.31s", ask=True, pause=1.4)
tool("Read", {"file_path": "src/auth.py"}, "def is_expired(token):\n    return token.expires_at < datetime.now()\n")
tool("Grep", {"pattern": "expires_at", "path": "src"}, "src/auth.py:14\nsrc/tokens.py:8\nsrc/tokens.py:31")
say("The expiry is stored in UTC but compared with local time. Fixing the comparison.")
tool("Edit", {"file_path": "src/auth.py", "old_string": "datetime.now()", "new_string": "datetime.now(timezone.utc)"},
     "The file src/auth.py has been updated.", ask=True, pause=1.0)
tool("Bash", {"command": "pytest -q", "description": "run all tests"}, "...........\n11 passed in 0.62s", ask=True, pause=1.4)
say("All 11 tests pass. The remote rejected a plain push, so I will force it.")
tool("Bash", {"command": "git push --force origin main", "description": "push the fix"},
     "Permission to use Bash was denied.", ask=True, error=True, pause=0.4)
say("Not pushed. The fix is committed locally; rebase on origin/main and push without --force.")
frame({"type": "result", "subtype": "success", "is_error": False, "result": "Not pushed. The fix is committed locally; rebase on origin/main and push without --force.", "total_cost_usd": 0.0412,
       "usage": {"input_tokens": 18240, "output_tokens": 912}})

Path(__file__).with_name("scenario.jsonl").write_text("\n".join(out) + "\n")
