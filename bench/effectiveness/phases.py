"""Turns by phase: where an agent's turns go (method:
sessions/contexts/turn-phase-decomposition.md).

Each top-level assistant turn is split evenly across the tool calls it made,
each call classified as:
  localize  find where to change: grep/rg/find/ls, Grep/Glob, graphite, Task
  read      read code: Read, cat/head/sed -n, git show/diff/log
  edit      Edit/Write, shell edits
  test      run tests/tools: pytest, uv, python, make, ruff
  other     anything else (TodoWrite, ...)
A turn with no tool call (the final answer) is `answer`. The ceiling a
locating tool can cut is the localize share.

Usage: python3 phases.py results/a.jsonl [results/b.jsonl ...] [--runs]
"""

import json
import sys
from collections import defaultdict
from pathlib import Path
from statistics import mean, median

from common import RESULTS
from stream import bash_category

PHASES = ("localize", "read", "edit", "test", "other", "answer")
BASH_PHASE = {"search": "localize", "list": "localize", "graphite": "localize",
              "read": "read", "git": "read", "edit": "edit", "run": "test", "other": "other"}
TOOL_PHASE = {"Grep": "localize", "Glob": "localize", "Task": "localize", "Agent": "localize",
              "Read": "read", "NotebookRead": "read",
              "Edit": "edit", "Write": "edit", "MultiEdit": "edit", "NotebookEdit": "edit"}


def call_phase(name: str, inp: dict) -> str:
    if name == "Bash":
        return BASH_PHASE[bash_category(inp.get("command", ""))]
    return TOOL_PHASE.get(name, "other")


def turn_phases(stream: Path) -> list[dict[str, float]]:
    """One {phase: weight} per top-level assistant turn, weights summing to 1."""
    turns: dict[str, list[str]] = {}
    order: list[str] = []
    for line in stream.read_text(errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") != "assistant" or ev.get("parent_tool_use_id") is not None:
            continue
        msg = ev.get("message") or {}
        mid = msg.get("id") or f"anon-{len(order)}"
        if mid not in turns:
            turns[mid] = []
            order.append(mid)
        for block in msg.get("content") or []:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                turns[mid].append(call_phase(block.get("name", "?"), block.get("input") or {}))
    out = []
    for mid in order:
        calls = turns[mid]
        if not calls:
            out.append({"answer": 1.0})
            continue
        w: dict[str, float] = defaultdict(float)
        for p in calls:
            w[p] += 1 / len(calls)
        out.append(dict(w))
    return out


def run_phases(record: dict) -> dict:
    stream = RESULTS / "runs" / record["run_id"] / "stream.jsonl"
    turns = turn_phases(stream)
    total = {p: round(sum(t.get(p, 0) for t in turns), 2) for p in PHASES}
    # Turns spent before the first edit: the "find and understand" stretch.
    first_edit = next((i for i, t in enumerate(turns) if t.get("edit")), len(turns))
    return {"run_id": record["run_id"], "task": record["task"], "arm": record["arm"],
            "label": record.get("label"), "statement": record.get("statement", "hinted"),
            "turns": len(turns), "num_turns": record.get("num_turns"),
            "turns_before_first_edit": first_edit, **total}


def load_records(paths: list[str]) -> list[dict]:
    recs = []
    for p in paths:
        for line in Path(p).read_text().splitlines():
            if line.strip():
                r = json.loads(line)
                if r.get("outcome") != "harness_error" and (RESULTS / "runs" / r["run_id"] / "stream.jsonl").exists():
                    recs.append(r)
    return recs


def summarize(rows: list[dict], key=lambda r: (r["label"], r["arm"])) -> list[dict]:
    groups: dict = defaultdict(list)
    for r in rows:
        groups[key(r)].append(r)
    out = []
    for k, rs in sorted(groups.items()):
        turns = sum(r["turns"] for r in rs)
        row = {"group": k, "runs": len(rs), "mean_turns": round(mean(r["turns"] for r in rs), 1),
               "median_turns_before_first_edit": median(r["turns_before_first_edit"] for r in rs)}
        for p in PHASES:
            row[f"{p}_turns"] = round(mean(r[p] for r in rs), 2)
            row[f"{p}_share"] = round(sum(r[p] for r in rs) / turns, 3) if turns else 0
        out.append(row)
    return out


def main() -> None:
    files = [a for a in sys.argv[1:] if not a.startswith("--")]
    rows = [run_phases(r) for r in load_records(files)]
    if "--runs" in sys.argv:
        for r in rows:
            print(json.dumps(r))
        return
    print("| group | runs | turns | before 1st edit (median) | "
          + " | ".join(PHASES) + " |")
    print("|---|---|---|---|" + "---|" * len(PHASES))
    for key in (lambda r: (r["label"], r["arm"]), lambda r: (r["label"], r["arm"], r["task"])):
        for s in summarize(rows, key):
            cells = " | ".join(f"{s[p + '_turns']} ({s[p + '_share']:.0%})" for p in PHASES)
            print(f"| {' / '.join(s['group'])} | {s['runs']} | {s['mean_turns']} | "
                  f"{s['median_turns_before_first_edit']} | {cells} |")


if __name__ == "__main__":
    main()
