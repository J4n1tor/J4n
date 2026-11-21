#!/usr/bin/env python3
"""Rebuild J4n's brand art in `assets/` from shapes, so every file can be made again and every size is its own clean
pixel grid (docs/BRAND.md).

    python3 scripts/brand/render.py --fonts DIR [--out assets]

The mark (the key ring), the janitor symbols and the shelf are drawn here with the standard library only: each is a
set of circles, lines, boxes and polygons on a 40-unit square, sampled at its native grid (16, 32, 40 or 48 cells) and
scaled up only by whole numbers. The banner and the lockups carry type, so they are laid out as HTML and photographed
by a headless Chromium; DIR must hold `RubikMonoOne-Regular.ttf` and `JetBrainsMono.ttf` (both SIL OFL, from Google
Fonts). Without Chromium or the fonts, only the pixel art is made.
"""

from __future__ import annotations

import argparse
import math
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

# The jjj palette: the TUI's default and the brand's (docs/BRAND.md §Colour).
GROUND = "#161a20"
TEXT = "#cdd2d8"
BRIGHT = "#e6eaee"
DIM = "#8b93a0"
ACCENT = "#7caede"
BRASS = "#d8bf7a"
STEEL = "#c3cbd4"
RING = "#9aa4b0"
LINE = "#2a313c"
# On a light page: the same roles, darker.
LIGHT_TEXT = "#24292f"
LIGHT_ACCENT = "#2c6aa3"
LIGHT_DIM = "#59636e"

HOLE = "x"  # a shape of this colour cuts a hole


# ---------------------------------------------------------------- drawing


def rgb(c: str) -> tuple[int, int, int]:
    n = int(c[1:], 16)
    return (n >> 16) & 255, (n >> 8) & 255, n & 255


def shade(c: str, k: float) -> str:
    """Darker (k < 0) or lighter (k > 0) by the share k."""

    def f(v: int) -> int:
        w = v * (1 + k) if k < 0 else v + (255 - v) * k
        return round(max(0, min(255, w)))

    r, g, b = rgb(c)
    return "#%02x%02x%02x" % (f(r), f(g), f(b))


def hit(s: tuple, x: float, y: float) -> bool:
    kind = s[0]
    if kind == "rect":
        return s[2] <= x <= s[4] and s[3] <= y <= s[5]
    if kind == "circ":
        return (x - s[2]) ** 2 + (y - s[3]) ** 2 <= s[4] ** 2
    if kind == "line":
        vx, vy, wx, wy = s[4] - s[2], s[5] - s[3], x - s[2], y - s[3]
        ln = vx * vx + vy * vy
        t = max(0.0, min(1.0, (wx * vx + wy * vy) / ln)) if ln else 0.0
        dx, dy = s[2] + t * vx - x, s[3] + t * vy - y
        return dx * dx + dy * dy <= s[6] ** 2
    if kind == "poly":
        p, inside, j = s[2], False, len(s[2]) - 1
        for i in range(len(p)):
            if (p[i][1] > y) != (p[j][1] > y) and x < (p[j][0] - p[i][0]) * (y - p[i][1]) / (p[j][1] - p[i][1]) + p[i][0]:
                inside = not inside
            j = i
        return inside
    return False


Grid = list[list[str | None]]


def raster(shapes: list[tuple], n: int) -> Grid:
    """Sample the 40-unit drawing at n×n cells; from 24 cells on, bottom/right edges shade darker and top/left catch light."""
    u = 40 / n
    g: Grid = []
    for y in range(n):
        row: list[str | None] = []
        for x in range(n):
            px, py, c = (x + 0.5) * u, (y + 0.5) * u, None
            for s in shapes:
                if hit(s, px, py):
                    c = s[1]
            row.append(None if c == HOLE else c)
        g.append(row)
    if n < 24:
        return g
    out: Grid = []
    for y in range(n):
        row = []
        for x in range(n):
            c = g[y][x]
            if not c:
                row.append(None)
                continue
            up = g[y - 1][x] if y > 0 else None
            dn = g[y + 1][x] if y < n - 1 else None
            lf = g[y][x - 1] if x > 0 else None
            rt = g[y][x + 1] if x < n - 1 else None
            if not dn or not rt:
                row.append(shade(c, -0.4))
            elif not up or not lf:
                row.append(shade(c, 0.3))
            else:
                row.append(c)
        out.append(row)
    return out


def key(a: float, c: str, length: float) -> list[tuple]:
    """One key hanging from the ring at angle a: bow with a hole, collar, shaft and three teeth."""
    p, d, nn = (20, 14), (math.sin(a), math.cos(a)), (math.cos(a), -math.sin(a))

    def at(u: float, v: float) -> tuple[float, float]:
        return p[0] + d[0] * u + nn[0] * v, p[1] + d[1] * u + nn[1] * v

    b = at(5, 0)
    s: list[tuple] = [("circ", c, *b, 4.3), ("circ", HOLE, *b, 1.7)]
    s.append(("line", c, *at(9.2, -2.4), *at(9.2, 2.4), 0.9))
    s.append(("line", c, *at(8.5, 0), *at(length, 0), 1.25))
    for i, uu in enumerate((length - 7, length - 4.5, length - 2)):
        s.append(("line", c, *at(uu, 0), *at(uu, 2.4 if i == 1 else 3.4), 0.95))
    return s


def mark(accent: str = ACCENT) -> list[tuple]:
    """The key ring: three keys (brass, steel, the accent) on one ring, no tag."""
    ring = [("circ", RING, 20, 7.5, 6.6), ("circ", HOLE, 20, 7.5, 4.8)]
    return ring + key(-0.5, BRASS, 23) + key(0.5, STEEL, 22) + key(0, accent, 25)


def symbols(a: str = ACCENT) -> dict[str, list[tuple]]:
    """Things from the janitor's cart. Decoration only: none of them stands for a feature."""
    y, yd, s, g, d, t = "#e0c070", "#b89a45", STEEL, RING, GROUND, BRIGHT
    sponge_green = "#83c795"
    return {
        "wet-floor": [
            ("poly", y, [(12, 5), (28, 5), (34, 37), (6, 37)]),
            ("rect", yd, 15, 2, 25, 5.5), ("rect", HOLE, 17, 3, 23, 4.6),
            ("rect", d, 18.4, 11, 21.6, 25), ("rect", d, 18.4, 28, 21.6, 31.5),
            ("line", a, 1, 38.6, 13, 38.6, 1), ("line", a, 27, 38.6, 39, 38.6, 1),
        ],
        "spray": [
            ("poly", a, [(12, 17), (26, 17), (28, 37), (10, 37)]), ("rect", t, 13, 23, 25, 30),
            ("rect", s, 16, 12, 22, 17), ("poly", g, [(12, 6), (25, 6), (25, 12), (14, 12)]),
            ("rect", g, 6, 7, 12, 9.5), ("line", g, 15, 12, 12, 18, 1),
            ("circ", t, 3, 4, 0.9), ("circ", t, 2, 8.5, 1), ("circ", t, 4, 12.5, 0.8),
        ],
        "bucket": [
            ("line", "#e2a373", 15, 1, 20, 24, 1.2), ("poly", y, [(8, 19), (32, 19), (29, 35), (11, 35)]),
            ("rect", a, 9, 19, 31, 21.5), ("rect", yd, 7, 17.5, 33, 19.4), ("rect", g, 23, 12, 34, 17.5),
            ("circ", "#5c6470", 12.5, 36.5, 2), ("circ", "#5c6470", 27.5, 36.5, 2),
        ],
        "bin": [
            ("rect", g, 16.5, 5.5, 23.5, 9), ("rect", HOLE, 18, 7, 22, 9), ("rect", g, 8, 9, 32, 13),
            ("poly", DIM, [(10, 13.5), (30, 13.5), (28, 37), (12, 37)]),
            ("line", "#5c6470", 15.5, 17, 16, 33.5, 0.6), ("line", "#5c6470", 20, 17, 20, 33.5, 0.6),
            ("line", "#5c6470", 24.5, 17, 24, 33.5, 0.6), ("rect", a, 27, 1.5, 30, 4.5), ("rect", a, 32, 4.5, 34, 6.5),
        ],
        "wrench": [
            ("line", s, 11, 29, 27, 13, 2.6), ("circ", s, 29, 11, 6), ("circ", HOLE, 32.5, 7.5, 3.2),
            ("circ", s, 10, 30, 3.4), ("circ", HOLE, 10, 30, 1.4), ("rect", a, 4, 8, 6, 10), ("rect", a, 7, 4.5, 8.5, 6),
        ],
        "sponge": [
            ("rect", y, 8, 19, 32, 33), ("rect", sponge_green, 8, 15, 32, 19),
            ("circ", yd, 13, 24, 1), ("circ", yd, 19, 28, 1.1), ("circ", yd, 25, 23, 0.9), ("circ", yd, 28, 29, 1),
            ("circ", a, 12, 8, 4), ("circ", HOLE, 12, 8, 2.8), ("circ", a, 23, 6, 2.8), ("circ", HOLE, 23, 6, 1.8),
            ("circ", a, 31, 11, 2), ("circ", HOLE, 31, 11, 1.1),
        ],
        "padlock": [
            ("circ", g, 20, 14, 8), ("circ", HOLE, 20, 14, 5.2), ("rect", HOLE, 0, 15, 40, 40), ("rect", g, 12, 13, 14.8, 20),
            ("rect", g, 25.2, 13, 28, 20), ("rect", y, 9, 19, 31, 36), ("circ", d, 20, 25.5, 2), ("rect", d, 19.2, 26, 20.8, 31),
        ],
        "badge": [
            ("line", a, 12, 0, 18, 10, 1), ("line", a, 28, 0, 22, 10, 1), ("rect", g, 17, 9, 23, 13), ("rect", t, 9, 13, 31, 37),
            ("rect", a, 12, 17, 19, 25), ("rect", g, 21, 18, 28, 19.5), ("rect", g, 21, 22, 27, 23.5),
            ("rect", "#5c6470", 12, 29, 28, 31), ("rect", "#5c6470", 12, 33, 24, 34.5),
        ],
        "receipt": [
            ("poly", t, [(10, 4), (30, 4), (30, 34), (27, 37), (24, 34), (21, 37), (18, 34), (15, 37), (12, 34), (10, 36)]),
            ("rect", g, 13, 9, 27, 10.5), ("rect", g, 13, 13, 24, 14.5), ("rect", g, 13, 17, 26, 18.5), ("rect", d, 13, 24, 27, 26.5),
            ("circ", y, 30, 31, 6), ("circ", yd, 30, 31, 4.2), ("rect", y, 29.2, 27.5, 30.8, 34.5),
        ],
        "clipboard": [
            ("rect", "#e2a373", 9, 6, 31, 37), ("rect", t, 12, 10, 28, 34), ("rect", g, 15, 3, 25, 9), ("rect", HOLE, 18, 4, 22, 6),
            ("rect", g, 15, 14, 25, 15.5), ("line", sponge_green, 15, 22.5, 16.6, 24.3, 0.7), ("line", sponge_green, 16.6, 24.3, 19.2, 20.8, 0.7),
            ("rect", g, 21, 22, 25.5, 23.5), ("line", sponge_green, 15, 29, 16.6, 30.8, 0.7), ("line", sponge_green, 16.6, 30.8, 19.2, 27.3, 0.7),
            ("rect", g, 21, 28.5, 25.5, 30),
        ],
        "flashlight": [
            ("poly", "#6b5f3a", [(27, 9), (38, 0), (40, 7), (31, 13)]), ("poly", y, [(28, 10), (36, 3), (38, 6), (30, 12)]),
            ("line", g, 8, 32, 22, 18, 3), ("line", s, 22, 18, 27, 13, 4.6), ("rect", a, 13, 23.5, 16, 26.5),
        ],
        "walkie-talkie": [
            ("rect", g, 23, 2, 25.5, 12), ("rect", g, 12, 11, 28, 37), ("rect", a, 15, 14, 25, 21), ("rect", d, 15, 24, 25, 25.2),
            ("rect", d, 15, 27, 25, 28.2), ("rect", d, 15, 30, 25, 31.2), ("rect", "#e2a373", 9, 17, 12, 24),
        ],
        "ladder": [
            ("line", "#e2a373", 12, 38, 15, 2, 1.2), ("line", "#e2a373", 28, 38, 25, 2, 1.2), ("line", y, 14.5, 8, 25.5, 8, 0.9),
            ("line", y, 13.9, 15, 26.1, 15, 0.9), ("line", y, 13.3, 22, 26.7, 22, 0.9), ("line", y, 12.75, 29, 27.25, 29, 0.9),
            ("line", y, 12.2, 36, 27.8, 36, 0.9), ("circ", a, 20, 3, 1.8),
        ],
        "toolbox": [
            ("rect", g, 14, 8, 26, 13), ("rect", HOLE, 16, 10, 24, 13), ("rect", "#e07e79", 6, 13, 34, 35), ("rect", "#b85f5a", 6, 19, 34, 21),
            ("rect", g, 18, 18, 22, 23), ("rect", a, 9, 25, 12, 31), ("rect", y, 28, 25, 31, 31),
        ],
        "binoculars": [
            ("rect", s, 7, 12, 17, 22), ("rect", s, 23, 12, 33, 22), ("rect", g, 16, 16, 24, 23), ("circ", g, 12, 26, 7.5),
            ("circ", g, 28, 26, 7.5), ("circ", a, 12, 26, 4.6), ("circ", a, 28, 26, 4.6), ("circ", t, 10.5, 24.5, 1.1), ("circ", t, 26.5, 24.5, 1.1),
        ],
    }


# At 16 cells the sampled bows run into one patch, so the smallest mark is drawn by hand: ring, then three bows each
# with its hole, the brass and steel shafts leaning out, the teeth on the outer side.
MARK_16 = [
    "......oooo......",
    ".....o....o.....",
    ".....o....o.....",
    "......oooo......",
    "...bbb.aaa.sss..",
    "...b.b.a.a.s.s..",
    "...bbb.aaa.sss..",
    "...b....a....s..",
    "...b....a....s..",
    "..bb....aa...ss.",
    "..b.....a.....s.",
    ".bb.....aa....ss",
    ".b......a......s",
    "........a.......",
    "........aa......",
    "................",
]


def mark_16(accent: str = ACCENT) -> Grid:
    colours = {"o": RING, "b": BRASS, "s": STEEL, "a": accent}
    return [[colours.get(ch) for ch in row] for row in MARK_16]


SHELF = ["wet-floor", "spray", "bucket", "bin", "wrench", "sponge"]


def dither(cols: int, rows: int, accent: str) -> Grid:
    """The banner's sweep: an ordered (Bayer 4×4) dither, denser to the right, its last columns in the accent."""
    bayer = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]
    out: Grid = []
    for y in range(rows):
        out.append([(accent if x > cols - 9 else LINE) if (x / (cols - 1)) ** 1.5 * 16 > bayer[y % 4][x % 4] else None for x in range(cols)])
    return out


def side_by_side(grids: list[Grid], gap: int) -> Grid:
    h = max(len(g) for g in grids)
    out: Grid = [[] for _ in range(h)]
    for i, g in enumerate(grids):
        for y in range(h):
            out[y].extend(g[y] if y < len(g) else [None] * len(g[0]))
            if i < len(grids) - 1:
                out[y].extend([None] * gap)
    return out


# ---------------------------------------------------------------- writing


def svg(g: Grid, title: str, cell: int = 1) -> str:
    """Crisp pixel SVG: one rect per horizontal run of one colour."""
    h, w = len(g), len(g[0])
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w * cell}" height="{h * cell}" viewBox="0 0 {w} {h}" '
        f'shape-rendering="crispEdges" role="img" aria-label="{title}"><title>{title}</title>'
    ]
    for y, row in enumerate(g):
        x = 0
        while x < w:
            c = row[x]
            if not c:
                x += 1
                continue
            e = x
            while e + 1 < w and row[e + 1] == c:
                e += 1
            parts.append(f'<rect x="{x}" y="{y}" width="{e - x + 1}" height="1" fill="{c}"/>')
            x = e + 1
    parts.append("</svg>\n")
    return "".join(parts)


def png(g: Grid, scale: int, ground: str | None = None, pad: int = 0, opacity: float = 1.0) -> bytes:
    """RGBA PNG with no metadata chunks: each cell a scale×scale block, pad cells of ground (or clear) around."""
    h, w = len(g), len(g[0])
    bg = (*rgb(ground), 255) if ground else (0, 0, 0, 0)
    alpha = round(255 * opacity)
    width, height = (w + 2 * pad) * scale, (h + 2 * pad) * scale
    raw = bytearray()
    blank = bytes(bg) * width
    for _ in range(pad * scale):
        raw += b"\0" + blank
    for row in g:
        line = bytearray(bytes(bg) * (pad * scale))
        for c in row:
            px = bytes((*rgb(c), alpha)) if c else bytes(bg)
            line += px * scale
        line += bytes(bg) * (pad * scale)
        for _ in range(scale):
            raw += b"\0" + line
    for _ in range(pad * scale):
        raw += b"\0" + blank

    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    head = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def put(path: Path, data: str | bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if isinstance(data, str):
        path.write_text(data)
    else:
        path.write_bytes(data)
    print(f"  {path}")


# ---------------------------------------------------------------- type, through a browser


def svg_inline(g: Grid, px: int, extra: str = "") -> str:
    s = svg(g, "")
    h, w = len(g), len(g[0])
    return s.replace(f'width="{w}" height="{h}"', f'width="{w * px}" height="{h * px}" {extra}', 1).replace(
        ' role="img" aria-label=""><title></title>', ' aria-hidden="true">', 1
    )


def faces(fonts: Path) -> str:
    return (
        "@font-face{font-family:'Rubik Mono One';src:url('%s') format('truetype')}"
        "@font-face{font-family:'JetBrains Mono';src:url('%s') format('truetype');font-weight:100 800}"
        % ((fonts / "RubikMonoOne-Regular.ttf").as_uri(), (fonts / "JetBrainsMono.ttf").as_uri())
    )


HEADLINE = "Your personal janitor for coding agents."
SUBLINE = "One place to control them all. Customize it, carve out your own orchestrator, and let it evolve."


def banner_html(fonts: Path) -> str:
    sweep = svg_inline(dither(44, 80, ACCENT), 8, 'style="position:absolute;right:0;top:0"')
    ring = svg_inline(raster(mark(), 40), 3)
    shelf = "".join(svg_inline(raster(symbols()[k], 32), 2, 'style="opacity:.85"') for k in SHELF)
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>{faces(fonts)}
html,body{{margin:0;background:{GROUND}}}
.b{{position:relative;width:1280px;height:640px;overflow:hidden;background:{GROUND};font-family:'JetBrains Mono',monospace}}
.l{{position:absolute;left:80px;top:0;bottom:0;width:760px;display:flex;flex-direction:column;justify-content:center;gap:26px}}
.m{{display:flex;align-items:flex-end;gap:28px}} .w{{display:flex;flex-direction:column;gap:6px;margin-bottom:4px}}
.j{{font-family:'Rubik Mono One';font-size:76px;line-height:.95;color:{BRIGHT}}} .j span{{color:{ACCENT}}}
.f{{font-family:'Rubik Mono One';font-size:18px;color:{DIM};letter-spacing:2px}}
.h{{font-size:44px;font-weight:700;line-height:1.15;color:{BRIGHT}}} .s{{font-size:19px;line-height:1.5;color:#9aa2ad}}
.t{{display:flex;gap:18px;align-items:center;margin-top:6px}}
</style></head><body><div class="b">{sweep}<div class="l">
<div class="m">{ring}<div class="w"><div class="j">J<span>4</span>n</div><div class="f">J4n1tor</div></div></div>
<div class="h">{HEADLINE}</div><div class="s">{SUBLINE}</div><div class="t">{shelf}</div>
</div></div></body></html>"""


def lockup_html(fonts: Path, light: bool) -> str:
    text, acc, dim = (LIGHT_TEXT, LIGHT_ACCENT, LIGHT_DIM) if light else (BRIGHT, ACCENT, DIM)
    ground = "transparent" if light else GROUND
    ring = svg_inline(raster(mark(acc), 48), 4)
    return f"""<!doctype html><html><head><meta charset="utf-8"><style>{faces(fonts)}
html,body{{margin:0;background:{ground}}}
.b{{width:760px;height:260px;box-sizing:border-box;padding:34px 40px;display:flex;align-items:center;gap:36px;background:{ground}}}
.w{{display:flex;flex-direction:column;gap:12px}}
.j{{font-family:'Rubik Mono One';font-size:118px;line-height:.9;color:{text}}} .j span{{color:{acc}}}
.f{{font-family:'Rubik Mono One';font-size:28px;color:{dim};letter-spacing:3px}}
</style></head><body><div class="b">{ring}<div class="w"><div class="j">J<span>4</span>n</div><div class="f">J4n1tor</div></div></div></body></html>"""


def shoot(chrome: str, html: str, out: Path, size: tuple[int, int], clear: bool, work: Path) -> None:
    page = work / (out.stem + ".html")
    page.write_text(html)
    shot = work / (out.stem + ".shot.png")
    cmd = [
        chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check",
        f"--user-data-dir={work / 'profile'}", "--force-device-scale-factor=1", f"--window-size={size[0]},{size[1]}",
        "--virtual-time-budget=4000", "--run-all-compositor-stages-before-draw", f"--screenshot={shot}",
    ]
    if clear:
        cmd.append("--default-background-color=00000000")
    subprocess.run(cmd + [page.as_uri()], check=True, capture_output=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    magick = shutil.which("magick")
    if magick:  # drop the timestamps and other text chunks a browser or encoder may add
        subprocess.run([magick, str(shot), "-strip", f"PNG32:{out}"], check=True)
    else:
        shutil.copyfile(shot, out)
    print(f"  {out}")


# ---------------------------------------------------------------- main


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", default="assets", type=Path)
    ap.add_argument("--fonts", type=Path, help="folder with RubikMonoOne-Regular.ttf and JetBrainsMono.ttf")
    args = ap.parse_args()
    out: Path = args.out

    print("mark")
    natives = {16: mark_16(), 32: raster(mark(), 32), 48: raster(mark(), 48)}
    for n, g in natives.items():
        put(out / f"mark-{n}.svg", svg(g, "J4n key ring"))
    put(out / "mark.svg", svg(natives[48], "J4n key ring"))
    # Each size from the largest native grid it is a whole multiple of: never resampled.
    for size, n in ((16, 16), (32, 32), (48, 48), (64, 32), (96, 48), (128, 32), (192, 48), (256, 32), (384, 48)):
        put(out / "png" / f"mark-{size}.png", png(natives[n], size // n))
    put(out / "png" / "avatar.png", png(natives[48], 10, ground=GROUND, pad=6))

    print("symbols")
    sym = symbols()
    for name, shapes in sym.items():
        put(out / "symbols" / f"{name}.svg", svg(raster(shapes, 32), name.replace("-", " ") + (" sign" if name == "wet-floor" else "")))
    shelf = side_by_side([raster(sym[k], 32) for k in SHELF], 12)
    put(out / "shelf.svg", svg(shelf, "janitor's shelf"))
    put(out / "png" / "shelf.png", png(shelf, 2))
    put(out / "png" / "wet-floor.png", png(raster(sym["wet-floor"], 32), 2))

    chrome = next((p for p in (shutil.which(c) for c in ("chromium", "chromium-browser", "google-chrome", "chrome")) if p), None)
    fonts = args.fonts
    if not chrome or not fonts or not (fonts / "RubikMonoOne-Regular.ttf").is_file() or not (fonts / "JetBrainsMono.ttf").is_file():
        print("banner and lockups skipped: they need Chromium and --fonts with both TTF files", file=sys.stderr)
        return 0
    print("type")
    with tempfile.TemporaryDirectory(prefix="j4n-brand-") as tmp:
        work = Path(tmp)
        shoot(chrome, banner_html(fonts.resolve()), out / "png" / "banner.png", (1280, 640), False, work)
        shoot(chrome, lockup_html(fonts.resolve(), False), out / "png" / "lockup-dark.png", (760, 260), False, work)
        shoot(chrome, lockup_html(fonts.resolve(), True), out / "png" / "lockup-light.png", (760, 260), True, work)
    return 0


if __name__ == "__main__":
    sys.exit(main())
