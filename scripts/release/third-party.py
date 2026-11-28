#!/usr/bin/env python3
"""Write docs/THIRD-PARTY.md: every crate the J4n binaries are built from, with its version, license and source.

Run from the repository root:  cargo metadata --format-version 1 --locked | python3 scripts/release/third-party.py
The licenses of these crates (MIT, Apache-2.0 and the like) ask that their notices go with the binaries: the release
workflow puts this file into each archive, beside LICENSE-MIT and LICENSE-APACHE.
"""
import json
import sys

meta = json.load(sys.stdin)
members = set(meta["workspace_members"])
used = {n["id"] for n in meta["resolve"]["nodes"]}
pkgs = sorted(
    (p for p in meta["packages"] if p["id"] in used and p["id"] not in members),
    key=lambda p: (p["name"].lower(), p["version"]),
)

out = [
    "# Third-party software in J4n",
    "",
    "J4n is licensed under MIT or Apache-2.0, at your option (`LICENSE-MIT`, `LICENSE-APACHE`). Its binaries are built from the crates below, each under its own license; the",
    "full text of each license is in the crate's source at the link given. `rusqlite` builds SQLite into the binary;",
    "SQLite is in the public domain (https://sqlite.org/copyright.html).",
    "",
    f"{len(pkgs)} crates. This file is made by `scripts/release/third-party.py`; do not edit it by hand.",
    "",
    "| crate | version | license | source |",
    "|---|---|---|---|",
]
for p in pkgs:
    lic = (p.get("license") or ("see " + str(p.get("license_file")))).replace("|", "\\|")
    src = p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}"
    out.append(f"| {p['name']} | {p['version']} | {lic} | {src} |")
print("\n".join(out))
