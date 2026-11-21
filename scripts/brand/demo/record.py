#!/usr/bin/env python3
"""Record the README's header GIF from the real `j4n` binary, with no model and no network.

    python3 scripts/brand/demo/record.py --bin target/release/j4n [--out assets/demo.gif]

It makes a throwaway home and project in a temp folder, starts `j4n` in a private tmux server (its own socket, 120×36)
with `J4N_CLAUDE` pointing at `player.sh`, which plays `scenario.jsonl` as a Claude Code session, and a dead proxy, so
nothing leaves the machine. Keys are typed on a timeline that waits for what the screen shows. Each changed screen is
captured with its colours, drawn as an SVG of cells in JetBrainsMono Nerd Font, rendered with `rsvg-convert`, and joined
by `ffmpeg` with each frame's own duration. Needs: tmux, git, rsvg-convert, ffmpeg, the font installed.
"""

from __future__ import annotations

import argparse
import html
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata
from pathlib import Path

HERE = Path(__file__).resolve().parent
COLS, ROWS = 120, 36
SOCK = "j4n-demo-record"
FG, BG = "#cdd2d8", "#161a20"
FONT = "JetBrainsMono Nerd Font"
CW, CH, FS, PAD = 9.6, 20, 16, 14
PROMPT = "the login test fails, fix it and push"
SPEED = 1.25  # played a little faster than it was recorded

AUTH = '''from datetime import datetime, timezone


def is_expired(token):
    return token.expires_at < datetime.now()
'''


def tmux(*args: str, check: bool = True) -> str:
    return subprocess.run(["tmux", "-L", SOCK, *args], capture_output=True, text=True, check=check).stdout


def setup(root: Path, binary: Path) -> None:
    home, shop = root / "home", root / "shop"
    (home / ".j4n" / "settings").mkdir(parents=True)
    (root / "tmp").mkdir()
    (shop / "src").mkdir(parents=True)
    (shop / "tests").mkdir()
    (shop / "src" / "auth.py").write_text(AUTH)
    (shop / "tests" / "test_login.py").write_text("def test_token_expires():\n    assert True\n")
    # The scripted session's edits are not real, so the project's own check after edits would test the untouched files.
    (home / ".j4n" / "settings" / "demo.toml").write_text('api = 1\n[[setting]]\nid = "check.after"\nvalue = false\n')
    claude = root / "claude"
    claude.write_text(f'#!/bin/sh\nFAKE_FIXTURE="{HERE / "scenario.jsonl"}" exec "{HERE / "player.sh"}" "$@"\n')
    claude.chmod(0o755)
    who = ["-c", "user.name=demo", "-c", "user.email=demo@example.com"]
    subprocess.run(["git", "init", "-q"], cwd=shop, check=True)
    subprocess.run(["git", "add", "-A"], cwd=shop, check=True)
    subprocess.run(["git", *who, "commit", "-q", "-m", "shop"], cwd=shop, check=True)
    tmux("kill-server", check=False)
    env = {
        "PATH": "/usr/bin:/bin", "HOME": str(home), "J4N_HOME": str(home / ".j4n"), "TMPDIR": str(root / "tmp"),
        "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "C.UTF-8", "https_proxy": "http://127.0.0.1:9",
        "J4N_CLAUDE": str(claude), "J4N_MOUSE": "0",
    }
    cmd = ["env", "-i", *[f"{k}={v}" for k, v in env.items()], str(binary), "--doer", "claude", "--model", "sonnet", "--approval", "ask"]
    tmux("-f", "/dev/null", "new-session", "-d", "-x", str(COLS), "-y", str(ROWS), "-c", str(shop), *cmd)
    tmux("set", "-g", "status", "off")


# ---------------------------------------------------------------- capture


class Recorder(threading.Thread):
    def __init__(self) -> None:
        super().__init__(daemon=True)
        self.frames: list[tuple[float, str]] = []
        self.stop = False
        self.t0 = time.monotonic()

    def screen(self) -> str:
        return tmux("capture-pane", "-p", "-e", "-t", "0", check=False)

    def run(self) -> None:
        while not self.stop:
            s = self.screen()
            if s and (not self.frames or self.frames[-1][1] != s):
                self.frames.append((time.monotonic() - self.t0, s))
            time.sleep(0.08)


def plain(s: str) -> str:
    return re.sub(r"\x1b\[[0-9;:]*m", "", s)


def until(rec: Recorder, text: str, limit: float = 30) -> None:
    end = time.monotonic() + limit
    while time.monotonic() < end:
        if text in plain(rec.screen()):
            return
        time.sleep(0.1)
    raise SystemExit(f"the screen never showed {text!r}")


def hold(s: float) -> None:
    time.sleep(s)


def key(*k: str) -> None:
    tmux("send-keys", "-t", "0", *k)


def type_line(text: str) -> None:
    for i in range(0, len(text), 3):
        tmux("send-keys", "-t", "0", "-l", text[i:i + 3])
        time.sleep(0.07)


def play(rec: Recorder) -> None:
    until(rec, "Settings")
    hold(1.0)
    type_line(PROMPT)
    hold(0.5)
    key("Enter")
    until(rec, "1–3 · ? why")  # the approval card for the edit
    hold(1.8)
    key("1")
    until(rec, "$0.0412")  # the turn has settled: the cost line
    hold(2.4)
    # Each page is held from when its content is drawn: the Log fills a moment after the tab opens.
    for k, shown, h in (("F2", "Not pushed", 1.6), ("F5", "usd", 1.6), ("F7", "gate.verdict", 2.4), ("F1", "without --force", 1.8)):
        key(k)
        until(rec, shown, 10)
        hold(h)


# ---------------------------------------------------------------- drawing


SGR = re.compile(r"\x1b\[([0-9;:]*)m")
BASIC = ["#1c2128", "#e07e79", "#83c795", "#d8bf7a", "#7caede", "#b199d7", "#7fc8c8", "#cdd2d8"]
BRIGHT = ["#5c6470", "#f0a19c", "#a4dcb2", "#e8d59c", "#a9c9ea", "#cbb8e8", "#a6e0e0", "#e6eaee"]


def xterm256(n: int) -> str:
    if n < 8:
        return BASIC[n]
    if n < 16:
        return BRIGHT[n - 8]
    if n < 232:
        n -= 16
        lv = [0, 95, 135, 175, 215, 255]
        return "#%02x%02x%02x" % (lv[n // 36], lv[(n // 6) % 6], lv[n % 6])
    v = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (v, v, v)


def width(ch: str) -> int:
    return 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1


def cells(screen: str) -> list[list[tuple[str, str, str, bool]]]:
    """Each row as cells of (char, fg, bg, bold); a wide char's second cell is an empty string."""
    out = []
    for line in screen.split("\n")[:ROWS]:
        fg, bg, bold, inv = FG, BG, False, False
        row: list[tuple[str, str, str, bool]] = []
        pos = 0
        for m in list(SGR.finditer(line)) + [None]:
            text = line[pos:m.start()] if m else line[pos:]
            for ch in text:
                f, b = (bg, fg) if inv else (fg, bg)
                row.append((ch, f, b, bold))
                if width(ch) == 2:
                    row.append(("", f, b, bold))
            if not m:
                break
            pos = m.end()
            ps = [int(p) if p else 0 for p in re.split("[;:]", m.group(1) or "0")]
            i = 0
            while i < len(ps):
                p = ps[i]
                if p == 0:
                    fg, bg, bold, inv = FG, BG, False, False
                elif p == 1:
                    bold = True
                elif p == 22:
                    bold = False
                elif p == 7:
                    inv = True
                elif p == 27:
                    inv = False
                elif 30 <= p <= 37:
                    fg = BASIC[p - 30]
                elif 90 <= p <= 97:
                    fg = BRIGHT[p - 90]
                elif 40 <= p <= 47:
                    bg = BASIC[p - 40]
                elif 100 <= p <= 107:
                    bg = BRIGHT[p - 100]
                elif p == 39:
                    fg = FG
                elif p == 49:
                    bg = BG
                elif p in (38, 48) and i + 1 < len(ps):
                    if ps[i + 1] == 2 and i + 4 < len(ps):
                        c = "#%02x%02x%02x" % tuple(ps[i + 2:i + 5])
                        i += 4
                    elif ps[i + 1] == 5 and i + 2 < len(ps):
                        c = xterm256(ps[i + 2])
                        i += 2
                    else:
                        c = FG if p == 38 else BG
                    if p == 38:
                        fg = c
                    else:
                        bg = c
                i += 1
        row = row[:COLS]
        row += [(" ", FG, BG, False)] * (COLS - len(row))
        out.append(row)
    while len(out) < ROWS:
        out.append([(" ", FG, BG, False)] * COLS)
    return out


def svg(screen: str) -> str:
    w, h = COLS * CW + 2 * PAD, ROWS * CH + 2 * PAD
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0f}" height="{h:.0f}">',
             f'<rect width="100%" height="100%" rx="10" fill="{BG}"/>']
    text_parts = []
    for y, row in enumerate(cells(screen)):
        top = PAD + y * CH
        x = 0
        while x < COLS:  # background runs
            b = row[x][2]
            e = x
            while e + 1 < COLS and row[e + 1][2] == b:
                e += 1
            if b != BG:
                parts.append(f'<rect x="{PAD + x * CW:.1f}" y="{top}" width="{(e - x + 1) * CW:.1f}" height="{CH}" fill="{b}"/>')
            x = e + 1
        x = 0
        while x < COLS:  # text runs of one style
            ch, f, _, bold = row[x]
            if ch in ("", " "):
                x += 1
                continue
            xs, chars = [], []
            while x < COLS and row[x][0] not in ("", " ") and row[x][1] == f and row[x][3] == bold:
                xs.append(f"{PAD + x * CW:.1f}")
                chars.append(html.escape(row[x][0]))
                x += 1
            weight = ' font-weight="bold"' if bold else ""
            text_parts.append(f'<text x="{" ".join(xs)}" y="{top + CH * 0.76:.1f}" fill="{f}"{weight}>{"".join(chars)}</text>')
    parts.append(f'<g font-family="{FONT}" font-size="{FS}" xml:space="preserve">')
    parts += text_parts
    parts.append("</g></svg>")
    return "".join(parts)


def render(frames: list[tuple[float, str]], out: Path, work: Path) -> None:
    lines = []
    for i, (t, s) in enumerate(frames):
        src, png = work / f"f{i:04d}.svg", work / f"f{i:04d}.png"
        src.write_text(svg(s))
        subprocess.run(["rsvg-convert", str(src), "-o", str(png)], check=True)
        dur = ((frames[i + 1][0] - t) if i + 1 < len(frames) else 2.5) / SPEED
        lines += [f"file '{png}'", f"duration {max(dur, 0.04):.3f}"]
    lines.append(f"file '{work / f'f{len(frames) - 1:04d}.png'}'")
    (work / "list.txt").write_text("\n".join(lines) + "\n")
    out.parent.mkdir(parents=True, exist_ok=True)
    graph = "split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(work / "list.txt"),
                    "-vf", graph, "-loop", "0", str(out)], check=True)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bin", required=True, type=Path)
    ap.add_argument("--out", default=Path("assets/demo.gif"), type=Path)
    ap.add_argument("--keep", type=Path, help="also keep the frames (svg, png) in this folder")
    args = ap.parse_args()
    for tool in ("tmux", "git", "rsvg-convert", "ffmpeg"):
        if not shutil.which(tool):
            print(f"needs {tool}", file=sys.stderr)
            return 1
    subprocess.run([sys.executable, "-I", str(HERE / "make_scenario.py")], check=True)
    with tempfile.TemporaryDirectory(prefix="j4n-demo-") as tmp:
        root = Path(tmp)
        setup(root, args.bin.resolve())
        rec = Recorder()
        rec.start()
        try:
            play(rec)
        finally:
            rec.stop = True
            rec.join()
            tmux("kill-server", check=False)
        frames = [(t - rec.frames[0][0], s) for t, s in rec.frames]
        work = args.keep or root / "frames"
        work.mkdir(parents=True, exist_ok=True)
        render(frames, args.out, work)
    print(f"{len(frames)} frames, {frames[-1][0]:.1f} s -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
