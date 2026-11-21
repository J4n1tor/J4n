#!/usr/bin/env python3
"""Leak scanner for the public side of J4n.

Scans files for things that must never be published: private terms (a private deny list), e-mail addresses,
real home folders, public IP addresses, and key or token shapes. It is run three ways:

  scripts/release/leakscan.py                 the public paths of this checkout (release/public.paths)
  scripts/release/leakscan.py --tree DIR      every file under DIR (the export, before it is committed)
  scripts/release/leakscan.py --staged        the files staged for commit (a pre-commit hook)

The private deny list is read from $J4N_DENY, else docs/dev/release-deny.txt when it exists. It is never exported,
so on the public side only the generic rules run. Exit status 1 on any finding.
"""
from __future__ import annotations

import argparse
import fnmatch
import ipaddress
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# Placeholder names a test or doc may put in a home folder path.
HOME_OK = {
    "user", "me", "you", "ana", "alice", "ada", "bob", "a", "b", "x", "u", "name", "runner", "someone", "dev",
    "j4n", "ci", "test", "tester", "example", "USER", "username", "*",
}
EMAIL_OK_DOMAINS = (
    "example.com", "example.org", "example.net", "localhost", "users.noreply.github.com", "noreply.github.com",
)
EMAIL_OK_SUFFIX = (".test", ".invalid", ".example", ".localhost", ".local")
EMAIL_OK = {"noreply@anthropic.com"}
IP_OK = {"1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4", "9.9.9.9", "93.184.216.34", "1.2.3.4", "0.0.0.0", "1.0.0.0",
         "255.255.255.255"}

SECRET_SHAPES = [
    ("anthropic key", re.compile(r"sk-ant-[A-Za-z0-9_-]{20,}")),
    ("openai key", re.compile(r"sk-(?:proj-)?[A-Za-z0-9]{32,}")),
    ("openrouter key", re.compile(r"sk-or-v1-[a-f0-9]{40,}")),
    ("github token", re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})")),
    ("aws key id", re.compile(r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b")),
    ("google key", re.compile(r"\bAIza[0-9A-Za-z_-]{35}\b")),
    ("slack token", re.compile(r"\bxox[abposr]-[A-Za-z0-9-]{10,}")),
    ("private key", re.compile(r"-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----")),
    ("jwt", re.compile(r"\beyJ[A-Za-z0-9_-]{15,}\.eyJ[A-Za-z0-9_-]{15,}\.[A-Za-z0-9_-]{10,}")),
    ("url with password", re.compile(r"[a-z][a-z0-9+.-]*://[^/\s:@'\"]+:([^@\s/'\"]{6,})@")),
]
# Traces of how J4n was made rather than how it works: row ids of the private decision and bug logs, the private projects
# and runs it was tried on, and the maintainer's own words given as a reason. Code says why in product terms instead.
TRACES = [
    ("decision row id", re.compile(r"\bD-\d{3,4}\b")),
    ("bug row id", re.compile(r"\bJ\d{2,4}\b|(?<=[\"/(-])j\d{2,4}\b")),
    ("private project", re.compile(r"\bJan\b|\bJ4n-(?:gym|eval|me)\b")),
    ("run note", re.compile(r"(?i)\blive run \d|\brun \d+[a-z]\b|\bthe gym\b")),
    ("maintainer's words", re.compile(r"(?i)\boperator\b[^\n]{0,40}\b20\d\d-[01]\d-[0-3]\d\b|(?<!::)\boperator[,:] [\"“]|\boperator's (?:first )?complaint")),
]
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}")
HOME = re.compile(r"(?<![\w.-])(?:/home/|/Users/|C:\\\\?Users\\\\?)([A-Za-z0-9._-]+)")
IPV4 = re.compile(r"(?<![\d.])((?:\d{1,3}\.){3}\d{1,3})(?![\d.])")


def load_list(path: Path) -> list[str]:
    if not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.append(line)
    return out


def deny_terms(root: Path) -> list[str]:
    env = os.environ.get("J4N_DENY")
    path = Path(env) if env else ROOT / "docs/dev/release-deny.txt"
    terms = [t.lower() for t in load_list(path)]
    # law 10's cut word: the backticked word on the `Cut` line of the private vision document
    vision = ROOT / "docs/dev/VISION.md"
    if vision.exists():
        for line in vision.read_text(encoding="utf-8").splitlines():
            m = re.match(r"Cut\b[^`]*`([^`]+)`", line)
            if m:
                terms.append(m.group(1).lower())
    return terms


def matches(rel: str, globs: list[str]) -> bool:
    return any(fnmatch.fnmatchcase(rel, g) for g in globs)


def public_files(root: Path) -> list[str]:
    """Tracked files that the export takes: release/public.paths includes, minus its `!` excludes."""
    tracked = subprocess.run(["git", "-C", str(root), "ls-files", "-z"], capture_output=True, check=True).stdout
    files = [f for f in tracked.decode().split("\0") if f]
    rules = load_list(root / "release/public.paths")
    inc = [r for r in rules if not r.startswith("!")]
    exc = [r[1:] for r in rules if r.startswith("!")]
    return [f for f in files if matches(f, inc) and not matches(f, exc) and (root / f).is_file()]


def ip_bad(text: str) -> bool:
    if text in IP_OK:
        return False
    try:
        ip = ipaddress.ip_address(text)
    except ValueError:
        return False  # a version number such as 1.2.300.4
    if any(int(p) > 255 for p in text.split(".")):
        return False
    return ip.is_global and not ip.is_multicast


def scan_file(rel: str, data: bytes, deny: list[str], fakes: list[str], traces_ok: list[str] = ()) -> list[str]:
    out = []
    if b"\0" in data[:8192]:
        return [f"{rel}: binary file (review it by hand, then list it in release/binaries.txt)"]
    text = data.decode("utf-8", errors="replace")
    fake_ok = matches(rel, fakes)
    trace_ok = matches(rel, list(traces_ok))
    for n, line in enumerate(text.splitlines(), 1):
        low = line.lower()
        for term in deny:
            if term in low:
                out.append(f"{rel}:{n}: private term `{term}`")
        for m in EMAIL.finditer(line):
            e = m.group(0)
            dom = e.split("@", 1)[1].lower()
            if e.lower() in EMAIL_OK or dom in EMAIL_OK_DOMAINS or dom.endswith(EMAIL_OK_DOMAINS) \
                    or dom.endswith(EMAIL_OK_SUFFIX) or fake_ok:
                continue
            # crate@version and git-ssh shapes are not addresses
            if re.fullmatch(r"[a-z0-9_-]+@\d.*", e) or e.startswith("git@"):
                continue
            out.append(f"{rel}:{n}: e-mail `{e}`")
        for m in HOME.finditer(line):
            if m.group(1) not in HOME_OK and not m.group(1).startswith(("$", "{", "<")):
                out.append(f"{rel}:{n}: home folder of `{m.group(1)}`")
        for m in IPV4.finditer(line):
            if ip_bad(m.group(1)) and not fake_ok:
                out.append(f"{rel}:{n}: public IP `{m.group(1)}`")
        if not trace_ok:
            for name, rx in TRACES:
                if m := rx.search(line):
                    out.append(f"{rel}:{n}: {name} `{m.group(0)}`")
        if not fake_ok:
            for name, rx in SECRET_SHAPES:
                if rx.search(line):
                    out.append(f"{rel}:{n}: {name} shape")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--tree", help="scan every file under this folder")
    ap.add_argument("--staged", action="store_true", help="scan the files staged for commit")
    ap.add_argument("--list", action="store_true", help="print the public paths, one per line, and scan nothing")
    ap.add_argument("files", nargs="*", help="scan just these files")
    a = ap.parse_args()
    if a.list:
        print("\n".join(public_files(ROOT)))
        return 0

    base = Path(a.tree).resolve() if a.tree else ROOT
    deny = deny_terms(base)
    fakes = load_list(ROOT / "release/fake-secrets.txt") or load_list(base / "release/fake-secrets.txt")
    if a.tree:
        rels = [str(p.relative_to(base)) for p in base.rglob("*") if p.is_file() and ".git" not in p.relative_to(base).parts]
    elif a.staged:
        names = subprocess.run(["git", "-C", str(ROOT), "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
                               capture_output=True, check=True).stdout.decode().split("\0")
        public = set(public_files(ROOT))
        rels = [f for f in names if f in public]
    elif a.files:
        rels = a.files
    else:
        rels = public_files(ROOT)

    binaries = load_list(ROOT / "release/binaries.txt")
    traces_ok = load_list(ROOT / "release/traces-ok.txt") or load_list(base / "release/traces-ok.txt")
    findings = []
    for rel in sorted(rels):
        if matches(rel, binaries):
            continue
        if a.staged:
            data = subprocess.run(["git", "-C", str(ROOT), "show", f":{rel}"], capture_output=True).stdout
        else:
            data = (base / rel).read_bytes()
        findings += scan_file(rel, data, deny, fakes, traces_ok)

    for f in findings:
        print(f)
    where = a.tree or ("staged" if a.staged else "public paths")
    note = "" if deny else " (no private deny list: generic rules only)"
    print(f"leakscan: {len(rels)} files in {where}, {len(findings)} findings{note}", file=sys.stderr)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
