#!/usr/bin/env python3
"""How often does the agent read a definition's body right after a Graphite answer named it?

For every run of a pilot: definitions listed in hook search answers (record items of class
`definition`), then every later read in the same run (`sed -n A,Bp`, `cat`, `head`, `nl`, Read
tool, `grep -A N` from a `def` line) whose range covers the definition line. A covered
definition is a follow-up Read that an inline body would have answered.

  python3 followup.py ../effectiveness/results/runs --pilot pilot-issue-C2
"""

from __future__ import annotations

import argparse
import ast
import json
import re
import shlex
from pathlib import Path

SED = re.compile(r"sed -n\s+'?(\d+),(\d+)p'?\s+(\S+\.py)")
WHOLE = re.compile(r"\b(?:cat|nl)(?:\s+-n)?\s+((?:\S+\.py\s*)+)")
HEAD = re.compile(r"\bhead\s+-(?:n\s*)?(\d+)\s+(\S+\.py)")


def tool_uses(stream: Path) -> list[dict]:
    out = []
    for line in stream.read_text().splitlines():
        e = json.loads(line)
        if e.get("type") != "assistant":
            continue
        for c in e["message"]["content"]:
            if c.get("type") == "tool_use":
                out.append(c)
    return out


def record_of(ev: dict) -> dict:
    r = ev.get("record")
    if isinstance(r, str):
        try:
            return ast.literal_eval(r)
        except (ValueError, SyntaxError):
            return {}
    return r or {}


def reads(use: dict) -> list[tuple[str, int, int]]:
    """(path, lo, hi) ranges a tool call prints; whole files as (path, 1, 10**9)."""
    if use["name"] == "Read":
        f = use["input"].get("file_path", "")
        lo = int(use["input"].get("offset") or 1)
        n = int(use["input"].get("limit") or 2000)
        return [(f, lo, lo + n)]
    if use["name"] != "Bash":
        return []
    cmd = use["input"].get("command", "")
    out = [(p, int(a), int(b)) for a, b, p in SED.findall(cmd)]
    for files in WHOLE.findall(cmd):
        out += [(p, 1, 10**9) for p in files.split()]
    out += [(p, 1, int(n)) for n, p in HEAD.findall(cmd)]
    return out


def same_file(a: str, b: str) -> bool:
    a, b = a.lstrip("./"), b.lstrip("./")
    return a.endswith(b) or b.endswith(a)


def run(run_dir: Path) -> dict:
    hooks = run_dir / "hooks.jsonl"
    stream = run_dir / "stream.jsonl"
    if not hooks.exists() or not stream.exists():
        return {}
    defs_by_turn: dict[str, list[tuple[str, int]]] = {}
    for line in hooks.read_text().splitlines():
        ev = json.loads(line)
        if ev.get("action") != "answer" or ev.get("kind") != "search":
            continue
        for it in record_of(ev).get("items", []):
            if it.get("class") == "definition" and not it.get("test"):
                defs_by_turn.setdefault(ev.get("turn_id", ""), []).append((it["path"], int(it["line"])))
    pending: list[tuple[str, int]] = []
    covered: list[tuple[str, int]] = []
    for use in tool_uses(stream):
        for path, lo, hi in reads(use):
            for d in list(pending):
                if same_file(d[0], path) and lo <= d[1] <= hi:
                    covered.append(d)
                    pending.remove(d)
        for d in defs_by_turn.get(use["id"], []):
            if d not in pending and d not in covered:
                pending.append(d)
    return {"defs": len(pending) + len(covered), "read_after": len(covered), "examples": covered[:5]}


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("runs")
    ap.add_argument("--pilot", required=True)
    a = ap.parse_args()
    tot_d = tot_r = 0
    for d in sorted(Path(a.runs).glob(f"{a.pilot}-*")):
        if not d.name.startswith(a.pilot + "-") or "-C-" not in d.name:
            continue
        r = run(d)
        if not r:
            continue
        tot_d += r["defs"]
        tot_r += r["read_after"]
        print(f"{d.name}: {r['read_after']}/{r['defs']} {r['examples']}")
    print(f"TOTAL definitions named by an answer: {tot_d}; read afterwards: {tot_r}")


if __name__ == "__main__":
    main()
